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

    #[arg(id = "executor.call_present_evms", long = "executor-call-present-evms", default_value_t = 50)]
    pub call_present_evms: usize,

    #[arg(id = "executor.call_past_evms", long = "executor-call-past-evms", default_value_t = 50)]
    pub call_past_evms: usize,

    #[arg(id = "executor.inspector_evms", long = "executor-inspector-evms", default_value_t = 50)]
    pub inspector_evms: usize,

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
            call_present_evms: 50,
            call_past_evms: 50,
            inspector_evms: 50,
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
    pub fn init(&self, storage: Arc<StratusStorage>, miner: Arc<Miner>) -> Arc<Executor> {
        let config = *self;
        tracing::info!(?config, "creating executor");

        let executor = Executor::new(storage, miner, config);
        Arc::new(executor)
    }
}
