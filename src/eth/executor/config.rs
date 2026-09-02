use std::str::FromStr;
use std::sync::Arc;

use clap::Parser;
use display_json::DebugAsJson;
use revm::primitives::hardfork::SpecId;

use crate::eth::executor::Executor;
use crate::eth::miner::Miner;
use crate::eth::storage::StratusStorage;

#[derive(Parser, DebugAsJson, Clone, serde::Serialize)]
pub struct ExecutorConfig {
    /// Chain ID of the network.
    #[arg(long = "executor-chain-id", alias = "chain-id", env = "EXECUTOR_CHAIN_ID")]
    pub executor_chain_id: u64,

    #[arg(long = "executor-call-present-evms", env = "EXECUTOR_CALL_PRESENT_EVMS", default_value_t = 50)]
    pub call_present_evms: usize,

    #[arg(long = "executor-call-past-evms", env = "EXECUTOR_CALL_PAST_EVMS", default_value_t = 50)]
    pub call_past_evms: usize,

    #[arg(long = "executor-inspector-evms", env = "EXECUTOR_INSPECTOR_EVMS", default_value_t = 50)]
    pub inspector_evms: usize,

    /// Should reject contract transactions and calls to accounts that are not contracts?
    #[arg(
        long = "executor-reject-not-contract",
        alias = "reject-not-contract",
        env = "EXECUTOR_REJECT_NOT_CONTRACT",
        default_value = "true"
    )]
    pub executor_reject_not_contract: bool,

    #[arg(long = "executor-evm-spec", env = "EXECUTOR_EVM_SPEC", default_value = "Prague", value_parser = parse_evm_spec)]
    pub executor_evm_spec: SpecId,

    /// Enable revmc JIT compilation of hot contract bytecode. Requires the `revmc` cargo feature.
    #[cfg(feature = "revmc")]
    #[arg(long = "executor-jit", env = "EXECUTOR_JIT", default_value = "true")]
    pub executor_jit: bool,

    /// Compile to AOT shared-library artifacts instead of in-memory JIT modules.
    #[cfg(feature = "revmc")]
    #[arg(long = "executor-jit-aot", env = "EXECUTOR_JIT_AOT", default_value = "true")]
    pub executor_jit_aot: bool,

    /// Compile synchronously on the first execution of each contract, instead of asynchronously
    /// after it becomes hot. Guarantees compiled execution from the second execution on, at the
    /// cost of a one-time compile stall on the first execution of every contract.
    #[cfg(feature = "revmc")]
    #[arg(long = "executor-jit-blocking", env = "EXECUTOR_JIT_BLOCKING", default_value = "false")]
    pub executor_jit_blocking: bool,

    /// Directory where AOT-compiled artifacts are persisted so they survive restarts.
    #[cfg(feature = "revmc")]
    #[arg(long = "executor-jit-store-path", env = "EXECUTOR_JIT_STORE_PATH", default_value = "/tmp/stratus-revmc-aot")]
    pub executor_jit_store_path: std::path::PathBuf,

    /// Number of executions before a contract is considered hot and compiled.
    #[cfg(feature = "revmc")]
    #[arg(long = "executor-jit-hot-threshold", env = "EXECUTOR_JIT_HOT_THRESHOLD")]
    pub executor_jit_hot_threshold: Option<usize>,
}

fn parse_evm_spec(input: &str) -> anyhow::Result<SpecId> {
    SpecId::from_str(input).map_err(|err| anyhow::anyhow!("unknown hard fork: {err:?}"))
}

impl ExecutorConfig {
    /// Initializes Executor.
    ///
    /// Note: Should be called only after async runtime is initialized.
    pub fn init(&self, storage: Arc<StratusStorage>, miner: Arc<Miner>) -> Arc<Executor> {
        let config = self.clone();
        tracing::info!(?config, "creating executor");

        let executor = Executor::new(storage, miner, config);
        Arc::new(executor)
    }
}
