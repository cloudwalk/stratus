use jsonrpsee::types::ErrorObjectOwned;

use crate::eth::primitives::Address;
use crate::eth::rpc::rpc_internal_error;

/// Errors that can occur when anything related to Ethereum is executing.
///
/// TODO: organize better by context or just move to anyhow/eyre.
#[derive(Debug, thiserror::Error, derive_new::new)]
pub enum EthError {
    // -------------------------------------------------------------------------
    // Input data
    // -------------------------------------------------------------------------
    #[error("Failed to parse field '{field}' with value '{value}'")]
    InvalidField { field: &'static str, value: String },

    #[error("Failed to select block because it is greater than current block number or block hash is invalid.")]
    InvalidBlockSelection,

    #[error("Transaction signer cannot be recovered. Check the transaction signature is valid.")]
    InvalidSigner,

    #[error("Transaction sent without chain id is not allowed.")]
    InvalidChainId,

    #[error("Transaction sent without gas price is not allowed.")]
    InvalidGasPrice,

    #[error("Transaction sent from zero address is not allowed.")]
    ZeroSigner,

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

    #[error("Failed to connect to Storage")]
    StorageConnectionError,

    // -------------------------------------------------------------------------
    // Bugs
    // -------------------------------------------------------------------------
    #[error("Bug: Contract was deployed, but no address was returned.")]
    DeploymentWithoutAddress,

    // -------------------------------------------------------------------------
    // Type Conversion
    // -------------------------------------------------------------------------
    #[error("Cannot convert value '{value}' from '{from}' to '{into}'")]
    StorageConvertError { value: String, from: String, into: String },
}

impl From<EthError> for ErrorObjectOwned {
    fn from(error: EthError) -> Self {
        rpc_internal_error(error.to_string())
    }
}
