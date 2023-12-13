use jsonrpsee::types::error::INTERNAL_ERROR_CODE;
use jsonrpsee::types::error::INTERNAL_ERROR_MSG;
use jsonrpsee::types::ErrorObjectOwned;

use crate::eth::primitives::Address;

/// Errors that can occur when EVM is executing.
#[derive(Debug, thiserror::Error)]
pub enum EthError {
    // -------------------------------------------------------------------------
    // Input data
    // -------------------------------------------------------------------------
    #[error("Failed to parse field '{field}' with value '{value}'")]
    Parsing { field: &'static str, value: String },

    #[error("Transaction sent from zero address is not allowed.")]
    ZeroSigner,

    #[error("Transaction signer cannot be recovered. Check the transaction signature is valid.")]
    UnrecoverableSigner,

    // -------------------------------------------------------------------------
    // EVM
    // -------------------------------------------------------------------------
    #[error("Account '{0}' was expected to be loaded by EVM, but it was not")]
    AccountNotLoaded(Address),

    #[error("Unexpected error with EVM bytecode. Check logs for more information.")]
    UnexpectedEvmError,

    // -------------------------------------------------------------------------
    // Storage
    // -------------------------------------------------------------------------
    #[error("Cannot persist EVM state because current storage state does not match expected previous state.")]
    StorageConflict,

    #[error("Unexpected error with EVM storage. Check logs for more information.")]
    UnexpectedStorageError,

    // -------------------------------------------------------------------------
    // Bugs
    // -------------------------------------------------------------------------
    #[error("Bug: Contract was deployed, but no address was returned.")]
    DeploymentWithoutAddress,
}

impl EthError {
    /// Create a `Parsing` error.
    pub fn parsing(field: &'static str, value: String) -> Self {
        Self::Parsing { field, value }
    }
}

impl From<EthError> for ErrorObjectOwned {
    fn from(_: EthError) -> Self {
        ErrorObjectOwned::owned(INTERNAL_ERROR_CODE, INTERNAL_ERROR_MSG, None::<String>)
    }
}
