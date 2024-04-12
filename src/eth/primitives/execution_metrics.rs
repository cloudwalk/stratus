#[derive(Debug, Default, derive_more::Add, derive_more::AddAssign)]
pub struct ExecutionMetrics {
    /// Number of account reads during EVM execution.
    pub account_reads: usize,

    /// Number of slot reads during EVM execution.
    pub slot_reads: usize,

    /// Number of slot reads during EVM execution that were cached with prefetch.
    pub slot_reads_cached: usize,
}
