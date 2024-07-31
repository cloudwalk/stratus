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
    #[arg(long = "chain-id", env = "EXECUTOR_CHAIN_ID")]
    pub chain_id: u64,

    /// Number of EVM instances to run.
    ///
    /// TODO: should be configured for each kind of EvmRoute instead of being a single value.
    #[arg(long = "evms", env = "EXECUTOR_EVMS")]
    pub num_evms: usize,

    /// EVM execution strategy.
    #[arg(long = "strategy", env = "EXECUTOR_STRATEGY", default_value = "serial")]
    pub strategy: ExecutorStrategy,

    /// Should reject contract transactions and calls to accounts that are not contracts?
    #[arg(long = "reject-not-contract", env = "EXECUTOR_REJECT_NOT_CONTRACT", default_value = "true")]
    pub reject_not_contract: bool,
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
