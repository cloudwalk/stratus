use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use display_json::DebugAsJson;

use crate::eth::miner::Miner;
use crate::eth::storage::StratusStorage;
use crate::ext::parse_duration;
use crate::GlobalState;
use crate::NodeMode;

// -----------------------------------------------------------------------------
// Config
// -----------------------------------------------------------------------------

#[derive(Parser, DebugAsJson, Clone, serde::Serialize)]
pub struct MinerConfig {
    /// Target block time.
    #[arg(long = "block-mode", env = "BLOCK_MODE", default_value = "automine")]
    pub block_mode: MinerMode,
}

impl MinerConfig {
    /// Inits [`Miner`] with the appropriate mining mode based on the node mode.
    pub fn init(&self, storage: Arc<StratusStorage>) -> anyhow::Result<Arc<Miner>> {
        tracing::info!(config = ?self, "creating block miner");

        let mode = match GlobalState::get_node_mode() {
            NodeMode::Follower => MinerMode::External,
            NodeMode::Leader => self.block_mode,
        };

        self.init_with_mode(mode, storage)
    }

    /// Inits [`Miner`] with a specific mining mode, regardless of node mode.
    pub fn init_with_mode(&self, mode: MinerMode, storage: Arc<StratusStorage>) -> anyhow::Result<Arc<Miner>> {
        tracing::info!(config = ?self, mode = ?mode, "creating block miner with specific mode");

        // create miner
        let miner = Miner::new(Arc::clone(&storage), mode);
        let miner = Arc::new(miner);

        // create genesis block and accounts if necessary
        #[cfg(feature = "dev")]
        {
            let genesis = storage.read_block(&crate::eth::primitives::BlockFilter::Number(crate::eth::primitives::BlockNumber::ZERO))?;
            if genesis.is_none() {
                storage.reset_to_genesis()?;
            }
        }

        // set block number
        storage.set_pending_block_number_as_next_if_not_set()?;

        // enable interval miner
        if mode.is_interval() {
            Arc::clone(&miner).spawn_interval_miner()?;
        }

        Ok(Arc::clone(&miner))
    }
}

// -----------------------------------------------------------------------------
// Mode
// -----------------------------------------------------------------------------

/// Indicates when the miner will mine new blocks.
#[derive(Debug, Clone, Copy, PartialEq, strum::EnumIs, serde::Serialize, serde::Deserialize)]
pub enum MinerMode {
    /// Mines a new block for each transaction execution.
    #[serde(rename = "automine")]
    Automine,

    /// Mines a new block at specified interval.
    #[serde(rename = "interval")]
    Interval(Duration),

    /// Does not automatically mines a new block. A call to `mine_*` must be executed to mine a new block.
    #[serde(rename = "external")]
    External,
}

impl MinerMode {
    /// Checks if the mode allow to mine new blocks.
    pub fn can_mine_new_blocks(&self) -> bool {
        match self {
            Self::Automine => true,
            Self::Interval(_) => true,
            Self::External => false,
        }
    }
}

impl FromStr for MinerMode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self, Self::Err> {
        match s {
            "automine" => Ok(Self::Automine),
            "external" => Ok(Self::External),
            s => {
                let block_time = parse_duration(s)?;
                Ok(Self::Interval(block_time))
            }
        }
    }
}
