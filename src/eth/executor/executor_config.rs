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
    #[arg(long = "executor-chain-id", alias = "chain-id", env = "EXECUTOR_CHAIN_ID")]
    pub executor_chain_id: u64,

    /// Number of EVM instances to run.
    #[arg(long = "executor-evms", alias = "evms", env = "EXECUTOR_EVMS", default_value = "30")]
    pub executor_evms: usize,

    #[arg(long = "executor-call-present-evms", env = "EXECUTOR_CALL_PRESENT_EVMS")]
    pub executor_call_present_evms: Option<usize>,

    #[arg(long = "executor-call-past-evms", env = "EXECUTOR_CALL_PAST_EVMS")]
    pub executor_call_past_evms: Option<usize>,

    #[arg(long = "executor-inspector-evms", env = "EXECUTOR_INSPECTOR_EVMS")]
    pub executor_inspector_evms: Option<usize>,

    /// EVM execution strategy.
    #[arg(long = "executor-strategy", alias = "strategy", env = "EXECUTOR_STRATEGY", default_value = "serial")]
    pub executor_strategy: ExecutorStrategy,

    /// Should reject contract transactions and calls to accounts that are not contracts?
    #[arg(
        long = "executor-reject-not-contract",
        alias = "reject-not-contract",
        env = "EXECUTOR_REJECT_NOT_CONTRACT",
        default_value = "true"
    )]
    pub executor_reject_not_contract: bool,
}

impl ExecutorConfig {
    /// Initializes Executor.
    ///
    /// Note: Should be called only after async runtime is initialized.
    pub fn init(&self, storage: Arc<StratusStorage>, miner: Arc<Miner>) -> Arc<Executor> {
        let mut config = self.clone();
        config.executor_evms = max(config.executor_evms, 1);
        tracing::info!(?config, "creating executor");

        let executor = Executor::new(storage, miner, config);
        Arc::new(executor)
    }
}
