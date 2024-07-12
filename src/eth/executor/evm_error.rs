use revm::primitives::EVMError;

#[derive(Debug, thiserror::Error)]
pub enum EvmError {
    /// Temporary catch-all error type for REVM errors.
    #[error("REVM: {0:?}")]
    Revm(EVMError<anyhow::Error>),
}
