use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use display_json::DebugAsJson;

use crate::eth::miner::Miner;
#[cfg(feature = "dev")]
use crate::eth::primitives::test_accounts;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::BlockNumber;
use crate::eth::relayer::ExternalRelayerClient;
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

    /// Generates genesis block on startup when it does not exist.
    #[arg(long = "enable-genesis", env = "ENABLE_GENESIS", default_value = "false")]
    pub enable_genesis: bool,

    /// Enables test accounts with max wei on startup.
    #[cfg(feature = "dev")]
    #[arg(long = "enable-test-accounts", env = "ENABLE_TEST_ACCOUNTS", default_value = "false")]
    pub enable_test_accounts: bool,
}

impl MinerConfig {
    /// Inits [`BlockMiner`] with external mining mode, ignoring the configured value.
    pub fn init_external_mode(&self, storage: Arc<StratusStorage>, relayer: Option<ExternalRelayerClient>) -> anyhow::Result<Arc<Miner>> {
        self.init_with_mode(MinerMode::External, storage, relayer)
    }

    /// Inits [`BlockMiner`] with the configured mining mode.
    pub fn init(&self, storage: Arc<StratusStorage>, relayer: Option<ExternalRelayerClient>) -> anyhow::Result<Arc<Miner>> {
        self.init_with_mode(self.block_mode, storage, relayer)
    }

    fn init_with_mode(&self, mode: MinerMode, storage: Arc<StratusStorage>, relayer: Option<ExternalRelayerClient>) -> anyhow::Result<Arc<Miner>> {
        tracing::info!(config = ?self, "creating block miner");

        // create miner
        let miner = Miner::new(Arc::clone(&storage), mode, relayer);
        let miner = Arc::new(miner);

        // enable genesis block
        if self.enable_genesis {
            let genesis = storage.read_block(&BlockFilter::Number(BlockNumber::ZERO))?;
            if genesis.is_none() {
                tracing::info!("enabling genesis block");
                miner.commit(Block::genesis())?;
            }
        }

        // enable test accounts
        #[cfg(feature = "dev")]
        if self.enable_test_accounts {
            let test_accounts = test_accounts();
            tracing::info!(accounts = ?test_accounts, "enabling test accounts");
            storage.save_accounts(test_accounts)?;
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
