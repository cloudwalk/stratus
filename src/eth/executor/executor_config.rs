use std::cmp::max;
use std::sync::Arc;

use clap::Parser;
use display_json::DebugAsJson;

use crate::eth::executor::Executor;
use crate::eth::executor::ExecutorStrategy;
use crate::eth::miner::Miner;
use crate::eth::storage::StratusStorage;

#[derive(Parser, DebugAsJson, Clone, serde::Serialize)]
pub struct ExecutorConfig {
    /// Chain ID of the network.
    #[arg(long = "chain-id", env = "CHAIN_ID")]
    pub chain_id: u64,

    /// Number of EVM instances to run.
    #[arg(long = "evms", env = "EVMS")]
    pub num_evms: usize,

    /// EVM execution strategy.
    #[arg(long = "strategy", env = "STRATEGY", default_value = "serial")]
    pub strategy: ExecutorStrategy,
}

impl ExecutorConfig {
    /// Initializes Executor.
    ///
    /// Note: Should be called only after async runtime is initialized.
    pub fn init(&self, storage: Arc<StratusStorage>, miner: Arc<Miner>) -> Arc<Executor> {
        let mut config = self.clone();
        config.num_evms = max(config.num_evms, 1);
        tracing::info!(?config, "creating executor");

        let executor = Executor::new(storage, miner, config);
        Arc::new(executor)
    }
}
