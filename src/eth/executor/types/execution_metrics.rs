use display_json::DebugAsJson;

#[derive(DebugAsJson, Clone, Copy, Default, PartialEq, Eq, derive_more::Add, derive_more::AddAssign, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct EvmExecutionMetrics {
    /// Number of account reads during EVM execution.
    pub account_reads: usize,

    /// Number of slot reads during EVM execution.
    pub slot_reads: usize,
}
