use std::sync::Arc;

use clap::Parser;
use display_json::DebugAsJson;

use crate::eth::miner::Miner;
use crate::eth::miner::MinerMode;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::BlockNumber;
use crate::eth::relayer::ExternalRelayerClient;
use crate::eth::storage::StratusStorage;

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
