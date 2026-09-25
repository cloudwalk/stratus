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

    /// Total number of EVM workers in the unified pool, shared by every execution kind.
    #[arg(id = "executor.evm_workers", long = "executor-evm-workers")]
    pub evm_workers: Option<usize>,

    /// Maximum number of concurrent call-present executions.
    #[arg(id = "executor.call_present_limit", long = "executor-call-present-limit")]
    pub call_present_limit: Option<usize>,

    /// Maximum number of concurrent call-past executions.
    #[arg(id = "executor.call_past_limit", long = "executor-call-past-limit")]
    pub call_past_limit: Option<usize>,

    /// Maximum number of concurrent inspector executions.
    #[arg(id = "executor.inspector_limit", long = "executor-inspector-limit")]
    pub inspector_limit: Option<usize>,

    /// Extra permits that any execution kind can borrow when its own limit is exhausted.
    #[arg(id = "executor.evm_flex_quota", long = "executor-evm-flex-quota", default_value_t = 0)]
    pub evm_flex_quota: usize,

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

    /// Deprecated alias of `executor.call_present_limit`.
    #[arg(id = "executor.call_present_evms", long = "executor-call-present-evms")]
    pub call_present_evms: Option<usize>,

    /// Deprecated alias of `executor.call_past_limit`.
    #[arg(id = "executor.call_past_evms", long = "executor-call-past-evms")]
    pub call_past_evms: Option<usize>,

    /// Deprecated alias of `executor.inspector_limit`.
    #[arg(id = "executor.inspector_evms", long = "executor-inspector-evms")]
    pub inspector_evms: Option<usize>,
}

#[cfg(test)]
impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            executor_chain_id: 0,
            evm_workers: None,
            call_present_limit: None,
            call_past_limit: None,
            inspector_limit: None,
            evm_flex_quota: 0,
            executor_reject_not_contract: true,
            executor_evm_spec: SpecId::PRAGUE,
            call_present_evms: None,
            call_past_evms: None,
            inspector_evms: None,
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
    /// Returns whether any deprecated per-kind pool size field is set
    /// (`call_present_evms`, `call_past_evms` or `inspector_evms`).
    pub fn has_deprecated_pool_sizes(&self) -> bool {
        self.call_present_evms.is_some() || self.call_past_evms.is_some() || self.inspector_evms.is_some()
    }

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
