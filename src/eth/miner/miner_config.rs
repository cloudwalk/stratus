use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use display_json::DebugAsJson;

use crate::eth::miner::Miner;
use crate::eth::storage::StratusStorage;
use crate::ext::parse_duration;

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
    /// Inits [`BlockMiner`] with external mining mode, ignoring the configured value.
    pub fn init_external_mode(&self, storage: Arc<StratusStorage>) -> anyhow::Result<Arc<Miner>> {
        self.init_with_mode(MinerMode::External, storage)
    }

    /// Inits [`BlockMiner`] with the configured mining mode.
    pub fn init(&self, storage: Arc<StratusStorage>) -> anyhow::Result<Arc<Miner>> {
        self.init_with_mode(self.block_mode, storage)
    }

    fn init_with_mode(&self, mode: MinerMode, storage: Arc<StratusStorage>) -> anyhow::Result<Arc<Miner>> {
        tracing::info!(config = ?self, "creating block miner");

        // create miner
        let miner = Miner::new(Arc::clone(&storage), mode);
        let miner = Arc::new(miner);

        // create genesis block and accounts if necessary
        #[cfg(feature = "dev")]
        {
            let genesis = storage.read_block(&crate::eth::primitives::BlockFilter::Number(crate::eth::primitives::BlockNumber::ZERO))?;
            if mode.can_mine_new_blocks() && genesis.is_none() {
                storage.reset_to_genesis()?;
            }
        }

        // set block number
        storage.set_pending_block_number_as_next_if_not_set()?;

        // enable interval miner
        if miner.mode().is_interval() {
            Arc::clone(&miner).spawn_interval_miner()?;
        }

        Ok(miner)
    }
}

// -----------------------------------------------------------------------------
// Mode
// -----------------------------------------------------------------------------

/// Indicates when the miner will mine new blocks.
#[derive(Debug, Clone, Copy, strum::EnumIs, serde::Serialize)]
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
