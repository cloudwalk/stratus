use std::str::FromStr;
use std::sync::Arc;

use clap::Parser;
use display_json::DebugAsJson;
use revm::primitives::hardfork::SpecId;
use stratus_macros::CliOverrides;

use crate::eth::executor::Executor;
use crate::eth::miner::Miner;
use crate::eth::storage::StratusStorage;

#[derive(Parser, DebugAsJson, Clone, serde::Deserialize, serde::Serialize, CliOverrides)]
#[serde(default, deny_unknown_fields)]
pub struct ExecutorConfig {
    /// Chain ID of the network.
    #[arg(long = "executor-chain-id", alias = "chain-id", default_value = "0")]
    #[serde(rename = "chain_id")]
    pub executor_chain_id: u64,

    #[arg(long = "executor-call-present-evms", default_value_t = 50)]
    pub call_present_evms: usize,

    #[arg(long = "executor-call-past-evms", default_value_t = 50)]
    pub call_past_evms: usize,

    #[arg(long = "executor-inspector-evms", default_value_t = 50)]
    pub inspector_evms: usize,

    /// Should reject contract transactions and calls to accounts that are not contracts?
    #[arg(long = "executor-reject-not-contract", alias = "reject-not-contract", default_value = "true")]
    #[serde(rename = "reject_not_contract")]
    pub executor_reject_not_contract: bool,

    #[arg(long = "executor-evm-spec", default_value = "Prague", value_parser = parse_evm_spec)]
    #[serde(rename = "evm_spec", with = "spec_id_serde")]
    pub executor_evm_spec: SpecId,
}

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

/// Serde support for EVM hardfork specs using the same string form used by CLI arguments.
mod spec_id_serde {
    use revm::primitives::hardfork::SpecId;
    use serde::Deserialize;
    use serde::Deserializer;
    use serde::Serializer;

    pub fn serialize<S: Serializer>(spec: &SpecId, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&spec.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<SpecId, D::Error> {
        let value = String::deserialize(deserializer)?;
        super::parse_evm_spec(&value).map_err(serde::de::Error::custom)
    }
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
