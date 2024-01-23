use crate::eth::primitives::Address;
use anyhow::Context;
use anyhow::Result;

pub enum EthError {
    InvalidField { field: &'static str, value: String },
    InvalidBlockSelection,
    InvalidSigner,
    InvalidChainId,
    InvalidGasPrice,
    ZeroSigner,
    AccountNotLoaded(Address),
    UnexpectedEvmError,
    StorageConflict,
    UnexpectedStorageError,
    StorageConnectionError,
    DeploymentWithoutAddress,
    StorageConvertError { from: String, into: String },
}

impl From<EthError> for anyhow::Error {
    fn from(error: EthError) -> Self {
        match error {
            EthError::InvalidField { field, value } =>
                anyhow::anyhow!("Failed to parse field '{}' with value '{}'", field, value),
            EthError::InvalidBlockSelection =>
                anyhow::anyhow!("Failed to select block because it is greater than current block number or block hash is invalid."),
            EthError::InvalidSigner =>
                anyhow::anyhow!("Transaction signer cannot be recovered. Check the transaction signature is valid."),
            EthError::InvalidChainId =>
                anyhow::anyhow!("Transaction sent without chain id is not allowed."),
            EthError::InvalidGasPrice =>
                anyhow::anyhow!("Transaction sent without gas price is not allowed."),
            EthError::ZeroSigner =>
                anyhow::anyhow!("Transaction sent from zero address is not allowed."),
            EthError::AccountNotLoaded(address) =>
                anyhow::anyhow!("Account '{}' was expected to be loaded by EVM, but it was not", address),
            EthError::UnexpectedEvmError =>
                anyhow::anyhow!("Unexpected error with EVM bytecode. Check logs for more information."),
            EthError::StorageConflict =>
                anyhow::anyhow!("Cannot persist EVM state because current storage state does not match expected previous state."),
            EthError::UnexpectedStorageError => 
                anyhow::anyhow!("Unexpected error with EVM storage. Check logs for more information."),
            EthError::StorageConnectionError =>
                anyhow::anyhow!("Failed to connect to Storage"),
            EthError::DeploymentWithoutAddress =>
                anyhow::anyhow!("Bug: Contract was deployed, but no address was returned."),
            EthError::StorageConvertError { from, into } =>
                anyhow::anyhow!("Cannot convert from '{}' to '{}'", from, into),
        }
    }
}