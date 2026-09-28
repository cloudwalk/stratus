use std::str::FromStr;
use std::sync::Arc;

use clap::Parser;
use display_json::DebugAsJson;
use revm::primitives::hardfork::SpecId;

use crate::eth::executor::Executor;
use crate::eth::executor::evm_worker_pool::DEFAULT_BUSY_THRESHOLD;
use crate::eth::executor::evm_worker_pool::DEFAULT_KIND_LIMIT;
use crate::eth::executor::evm_worker_pool::DEFAULT_WORKERS;
use crate::eth::miner::Miner;
use crate::eth::storage::StratusStorage;

#[derive(Parser, DebugAsJson, Clone, Copy, serde::Serialize)]
pub struct ExecutorConfig {
    /// Chain ID of the network.
    #[arg(id = "executor.chain_id", long = "executor-chain-id", alias = "chain-id", value_parser = parse_chain_id)]
    #[serde(rename = "chain_id")]
    pub executor_chain_id: u64,

    /// Total number of EVM workers in the unified pool, shared by every execution kind.
    #[arg(id = "executor.evm_workers", long = "executor-evm-workers", default_value_t = DEFAULT_WORKERS)]
    pub evm_workers: usize,

    /// Maximum number of concurrent call-present executions.
    /// Defaults to the remaining pool capacity (`evm_workers` minus the other limits).
    #[arg(id = "executor.call_present_limit", long = "executor-call-present-limit")]
    pub call_present_limit: Option<usize>,

    /// Maximum number of concurrent call-past executions.
    #[arg(id = "executor.call_past_limit", long = "executor-call-past-limit", default_value_t = DEFAULT_KIND_LIMIT)]
    pub call_past_limit: usize,

    /// Maximum number of concurrent inspector executions.
    #[arg(id = "executor.inspector_limit", long = "executor-inspector-limit", default_value_t = DEFAULT_KIND_LIMIT)]
    pub inspector_limit: usize,

    /// Pool busy percentage above which per-kind limits are enforced: while the pool is below this
    /// threshold, tasks are admitted even above their kind's limit.
    #[arg(id = "executor.evm_busy_threshold", long = "executor-evm-busy-threshold", default_value_t = DEFAULT_BUSY_THRESHOLD)]
    pub evm_busy_threshold: usize,

    /// Should reject contract transactions and calls to accounts that are not contracts?
    #[arg(
        id = "executor.reject_not_contract",
        long = "executor-reject-not-contract",
        alias = "reject-not-contract",
        default_value = "true",
        default_missing_value = "true",
        action = clap::ArgAction::Set,
        num_args = 0..=1
    )]
    #[serde(rename = "reject_not_contract")]
    pub executor_reject_not_contract: bool,

    #[arg(id = "executor.evm_spec", long = "executor-evm-spec", default_value = "Prague", value_parser = parse_evm_spec)]
    #[serde(rename = "evm_spec", with = "spec_id_serde")]
    pub executor_evm_spec: SpecId,
}

#[cfg(test)]
impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            executor_chain_id: 0,
            evm_workers: DEFAULT_WORKERS,
            call_present_limit: None,
            call_past_limit: DEFAULT_KIND_LIMIT,
            inspector_limit: DEFAULT_KIND_LIMIT,
            evm_busy_threshold: DEFAULT_BUSY_THRESHOLD,
            executor_reject_not_contract: true,
            executor_evm_spec: SpecId::PRAGUE,
        }
    }
}

/// Serde support for serializing EVM hardfork specs using the same string form used by CLI arguments.
mod spec_id_serde {
    use revm::primitives::hardfork::SpecId;
    use serde::Serializer;

    pub fn serialize<S: Serializer>(spec: &SpecId, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&spec.to_string())
    }
}

/// Parses a chain id, rejecting zero: it was the "missing" sentinel before the argument became required.
fn parse_chain_id(input: &str) -> anyhow::Result<u64> {
    let chain_id = input.parse().map_err(|err| anyhow::anyhow!("invalid chain id \"{input}\": {err}"))?;
    if chain_id == 0 {
        return Err(anyhow::anyhow!("chain id cannot be zero"));
    }
    Ok(chain_id)
}

fn parse_evm_spec(input: &str) -> anyhow::Result<SpecId> {
    SpecId::from_str(input).map_err(|err| anyhow::anyhow!("unknown hard fork: {err:?}"))
}

impl ExecutorConfig {
    /// Initializes Executor.
    ///
    /// Note: Should be called only after async runtime is initialized.
    pub fn init(&self, storage: Arc<StratusStorage>, miner: Arc<Miner>) -> anyhow::Result<Arc<Executor>> {
        let config = *self;
        tracing::info!(?config, "creating executor");

        let executor = Executor::new(storage, miner, config)?;
        Ok(Arc::new(executor))
    }
}
