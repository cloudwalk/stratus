use display_json::DebugAsJson;

#[derive(DebugAsJson, Clone, Copy, Default, PartialEq, Eq, derive_more::Add, derive_more::AddAssign, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct EvmExecutionMetrics {
    /// Number of account reads during EVM execution.
    pub account_reads: usize,

    /// Number of slot reads during EVM execution.
    pub slot_reads: usize,
}
