//! Compatibility types for decoding a `Block` produced by a Stratus leader
//! running the `7c7d831` release.
//!
//! In that release `stratus_getBlockWithChanges` returns a JSON-serialized
//! primitive `Block` instead of a `BlockRocksdb`, and its `Block` wire format
//! predates the `execution_info` field: the data currently kept in
//! `TransactionExecution::execution_info` used to live directly inside
//! `evm_input`, and `ExecutionInfo::signer` was a plain `Address` instead of
//! the current `Signer` enum.
//!
//! These mirror structs decode the `7c7d831` wire format (reusing the current
//! primitive types, which are serialization-compatible) and restore a current
//! `Block` by rebuilding `execution_info` from `evm_input` — the same mapping
//! `7c7d831` itself used in `From<TransactionExecution> for TransactionInput`.
//! The resulting `Block` is then converted to `BlockRocksdb` through the
//! existing `From<Block> for BlockRocksdb`, yielding exactly what a `7c7d831`
//! leader would have produced had it returned `BlockRocksdb` directly.

use crate::eth::executor::EvmExecutionResult;
use crate::eth::executor::EvmInput;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockHeader;
use crate::eth::primitives::ExecutionInfo;
use crate::eth::primitives::MinedData;
use crate::eth::primitives::Signature;
use crate::eth::primitives::Signer;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionInfo;
use crate::eth::primitives::TransactionMined;

/// `7c7d831`-shaped `Block`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CompatBlock {
    pub header: BlockHeader,
    pub transactions: Vec<CompatTransactionMined>,
}

/// `7c7d831`-shaped `TransactionMined`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CompatTransactionMined {
    pub execution: CompatTransactionExecution,
    pub mined_data: MinedData,
}

/// `7c7d831`-shaped `TransactionExecution` (no `execution_info` field).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CompatTransactionExecution {
    pub info: TransactionInfo,
    pub signature: Signature,
    pub evm_input: EvmInput,
    pub result: EvmExecutionResult,
}

impl From<CompatBlock> for Block {
    fn from(item: CompatBlock) -> Self {
        Block {
            header: item.header,
            transactions: item.transactions.into_iter().map(TransactionMined::from).collect(),
        }
    }
}

impl From<CompatTransactionMined> for TransactionMined {
    fn from(item: CompatTransactionMined) -> Self {
        let CompatTransactionMined { execution, mined_data } = item;

        // Rebuild the `execution_info` that the current `TransactionExecution`
        // requires from the `evm_input` fields, mirroring how `7c7d831` built
        // it inside `From<TransactionExecution> for TransactionInput`. The
        // signer is wrapped in `Signer::Recovered` since the legacy format
        // stored the recovered `from` address directly.
        let execution_info = ExecutionInfo {
            chain_id: execution.evm_input.chain_id,
            nonce: execution.evm_input.nonce.unwrap_or_default(),
            signer: Signer::Recovered(execution.evm_input.from),
            to: execution.evm_input.to,
            value: execution.evm_input.value,
            input: execution.evm_input.data.clone(),
            gas_limit: execution.evm_input.gas_limit,
            gas_price: execution.evm_input.gas_price,
        };

        let restored_execution = TransactionExecution {
            info: execution.info,
            signature: execution.signature,
            execution_info,
            evm_input: execution.evm_input,
            result: execution.result,
        };

        TransactionMined {
            execution: restored_execution,
            mined_data,
        }
    }
}
