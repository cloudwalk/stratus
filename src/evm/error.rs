/// Errors that can occur when EVM is executing.
#[derive(Debug, thiserror::Error)]
pub enum EvmError {
    // -------------------------------------------------------------------------
    // Input data related
    // -------------------------------------------------------------------------
    #[error("Transaction sent from zero address is not allowed.")]
    TransactionFromZeroAddress,

    // -------------------------------------------------------------------------
    // Evm related
    // -------------------------------------------------------------------------
    #[error("Unexpected error with EVM bytecode. Check logs for more information.")]
    UnexpectedEvmError,

    // -------------------------------------------------------------------------
    // Storage related
    // -------------------------------------------------------------------------
    #[error("Cannot persist EVM state because storage state does not match expected previous state.")]
    StorageConflict,

    #[error("Unexpected error with EVM storage. Check logs for more information.")]
    UnexpectedStorageError,

    // -------------------------------------------------------------------------
    // Bugs
    // -------------------------------------------------------------------------
    #[error("Bug: Contract was deployed, but no address was returned.")]
    ContractDeploymentWithoutAddress,
}
