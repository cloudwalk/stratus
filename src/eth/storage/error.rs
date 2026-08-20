use stratus_macros::ErrorCode;

use crate::eth::executor::evm::types::TransactionExecutionInput;
use crate::eth::rpc::BlockFilter;
use crate::eth::types::BlockNumber;
use crate::eth::types::ErrorCode;

#[derive(Debug, thiserror::Error, strum::EnumProperty, strum::IntoStaticStr, ErrorCode)]
#[major_error_code = 3000]
pub enum StorageError {
    #[error("block conflict: {number} already exists in the permanent storage.")]
    #[error_code = 1]
    BlockConflict { number: BlockNumber },

    #[error("mined number conflict between new block number ({new}) and mined block number ({mined}).")]
    #[error_code = 2]
    MinedNumberConflict { new: BlockNumber, mined: BlockNumber },

    // *deprecated*
    // #[error("Transaction execution conflicts: {0:?}.")]
    // #[error_code = 3]
    // TransactionConflict(Box<ExecutionConflicts>),
    #[error("transaction input does not match block header")]
    #[error_code = 4]
    EvmInputMismatch {
        expected: Box<TransactionExecutionInput>,
        actual: Box<TransactionExecutionInput>,
    },

    #[error("pending number conflict between new block number ({new}) and pending block number ({pending}).")]
    #[error_code = 5]
    PendingNumberConflict { new: BlockNumber, pending: BlockNumber },

    #[error("there are ({pending_txs}) pending transactions.")]
    #[error_code = 6]
    PendingTransactionsExist { pending_txs: usize },

    #[error("rocksdb returned an error: {err}")]
    #[error_code = 7]
    RocksError { err: anyhow::Error },

    #[error("block not found using filter: {filter}")]
    #[error_code = 8]
    BlockNotFound { filter: BlockFilter },

    #[error("unexpected storage error: {msg}")]
    #[error_code = 9]
    Unexpected { msg: String },
}
