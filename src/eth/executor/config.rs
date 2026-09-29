use std::str::FromStr;
use std::sync::Arc;

use clap::Parser;
use display_json::DebugAsJson;
use revm::primitives::hardfork::SpecId;

use crate::eth::executor::Executor;
use crate::eth::miner::Miner;
use crate::eth::storage::StratusStorage;

#[derive(Parser, DebugAsJson, Clone, Copy, serde::Serialize)]
pub struct ExecutorConfig {
    /// Chain ID of the network.
    #[arg(id = "executor.chain_id", long = "executor-chain-id", alias = "chain-id", value_parser = parse_chain_id)]
    #[serde(rename = "chain_id")]
    pub executor_chain_id: u64,

    #[command(flatten)]
    #[serde(flatten)]
    pub pool: PoolConfig,

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

/// Configuration of the unified EVM worker pool: one shared set of workers serving every execution kind.
#[derive(Parser, DebugAsJson, Clone, Copy, serde::Serialize)]
pub struct PoolConfig {
    /// Total number of EVM workers in the unified pool, shared by every execution kind.
    #[arg(id = "executor.evm_workers", long = "executor-evm-workers", default_value_t = 150, value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..))]
    pub evm_workers: usize,

    /// Maximum number of concurrent call-present executions.
    /// Defaults to the remaining pool capacity (`evm_workers` minus the other limits).
    #[arg(id = "executor.call_present_limit", long = "executor-call-present-limit", value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_present_limit: Option<usize>,

    /// Maximum number of concurrent call-past executions.
    #[arg(id = "executor.call_past_limit", long = "executor-call-past-limit", default_value_t = 50, value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..))]
    pub call_past_limit: usize,

    /// Maximum number of concurrent inspector executions.
    #[arg(id = "executor.inspector_limit", long = "executor-inspector-limit", default_value_t = 50, value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..))]
    pub inspector_limit: usize,

    /// Pool busy percentage above which per-kind limits are enforced: while the pool is below this
    /// threshold, tasks are admitted even above their kind's limit.
    #[arg(id = "executor.evm_busy_threshold", long = "executor-evm-busy-threshold", default_value_t = 80, value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(0..=100))]
    pub evm_busy_threshold: usize,
}

impl PoolConfig {
    /// Effective call-present limit: the configured value, or the remaining pool capacity by default.
    pub fn call_present_limit(&self) -> usize {
        self.call_present_limit
            .unwrap_or_else(|| self.evm_workers.saturating_sub(self.call_past_limit + self.inspector_limit))
    }

    /// In-flight task count at which relaxed admission ends and per-kind limits are enforced.
    pub fn relaxed_limit(&self) -> usize {
        self.evm_workers * self.evm_busy_threshold / 100
    }

    /// Returns warnings about suboptimal configurations the pool can still run with.
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        let limits_sum = self.call_present_limit() + self.call_past_limit + self.inspector_limit;
        if limits_sum > self.evm_workers {
            warnings.push(format!(
                "executor pool kind limits ({}) exceed the total number of workers ({}); saturating every kind makes tasks queue instead of execute",
                limits_sum, self.evm_workers
            ));
        }

        if self.call_present_limit.is_none() && self.call_present_limit() == 0 {
            warnings.push("call-present limit defaults to zero; call-present tasks are only admitted while the pool is below the busy threshold".to_string());
        }

        warnings
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
    pub fn init(&self, storage: Arc<StratusStorage>, miner: Arc<Miner>) -> Arc<Executor> {
        let config = *self;
        tracing::info!(?config, "creating executor");

        let executor = Executor::new(storage, miner, config);
        Arc::new(executor)
    }
}
