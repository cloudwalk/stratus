use display_json::DebugAsJson;

#[derive(DebugAsJson, Clone, Copy, Default, derive_more::Add, derive_more::AddAssign, serde::Serialize)]
pub struct EvmExecutionMetrics {
    /// Number of account reads during EVM execution.
    pub account_reads: usize,

    /// Number of slot reads during EVM execution.
    pub slot_reads: usize,
}
