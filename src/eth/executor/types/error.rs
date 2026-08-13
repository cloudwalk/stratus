use revm::context::result::EVMError;
use revm::context::result::InvalidTransaction;
use stratus_macros::ErrorCode;

use crate::eth::executor::RevertReason;
use crate::eth::types::Address;
use crate::eth::types::Bytes;
use crate::eth::types::ErrorCode;
use crate::eth::types::Nonce;
use crate::eth::types::StratusError;

#[derive(Debug, thiserror::Error, strum::EnumProperty, strum::IntoStaticStr, ErrorCode)]
#[major_error_code = 2000]
pub enum ExecutorError {
    #[error("function selector was not recognized: account at {address} is not a contract.")]
    #[error_code = 1]
    AccountNotContract { address: Address },

    #[error("transaction nonce {transaction} does not match account nonce {account}.")]
    #[error_code = 2]
    Nonce { transaction: Nonce, account: Nonce },

    #[error("EVM execution error: {0:?}.")]
    #[error_code = 3]
    EvmFailed(String), // TODO: split this in multiple errors

    #[error("failed to execute transaction in leader: {0:?}.")]
    #[error_code = 4]
    LeaderFailed(jsonrpsee::types::ErrorObjectOwned),

    #[error("failed to forward transaction to leader node.")]
    #[error_code = 5]
    ForwardToLeaderFailed,

    #[error("transaction reverted during execution. output: {output}")]
    #[error_code = 6]
    RevertedCall { output: Bytes },

    #[error("transaction from zero address is not allowed.")]
    #[error_code = 7]
    FromZeroAddress,

    #[error("transaction reverted during execution. reason: {reason}")]
    #[error_code = 8]
    RevertedCallWithReason { reason: RevertReason },

    #[error("evm executor panicked, see logs for details")]
    #[error_code = 9]
    Panic { err: anyhow::Error },
}

impl From<EVMError<StratusError>> for StratusError {
    fn from(value: EVMError<StratusError>) -> Self {
        match value {
            // nonce errors
            EVMError::Transaction(InvalidTransaction::NonceTooHigh { tx, state }) => ExecutorError::Nonce {
                transaction: tx.into(),
                account: state.into(),
            }
            .into(),
            EVMError::Transaction(InvalidTransaction::NonceTooLow { tx, state }) => ExecutorError::Nonce {
                transaction: tx.into(),
                account: state.into(),
            }
            .into(),

            // storage error
            EVMError::Database(err) => err,

            // unexpected errors
            err => ExecutorError::EvmFailed(err.to_string()).into(),
        }
    }
}
