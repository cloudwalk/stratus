use display_json::DebugAsJson;

use crate::eth::types::Gas;

#[derive(DebugAsJson, Clone, Copy, Default, PartialEq, Eq, derive_more::Add, derive_more::AddAssign, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct SlotAccessMetrics {
    /// Number of account reads during EVM execution.
    pub account_reads: usize,

    /// Number of slot reads during EVM execution.
    pub slot_reads: usize,
}

pub struct EvmExecutionMetrics {
    pub slot_access: SlotAccessMetrics,
    pub gas_used: Gas,
}
