//! Compatibility types for decoding a `Block` produced by a Stratus leader
//! running the `7c7d831` release.
//!
//! In that release `stratus_getBlockWithChanges` returns a JSON-serialized
//! primitive `Block` instead of a `BlockRocksdb`, and its `Block` wire format
//! predates two changes that exist in the current codebase:
//!
//! 1. `TransactionExecution` gained an `execution_info` field. In `7c7d831` the
//!    same data lived directly inside `evm_input` (and `ExecutionInfo.signer`
//!    was a plain `Address` instead of the current `Signer` enum).
//! 2. `ExecutionChanges` gained a `_stage: PhantomData<Stage>` field that, due
//!    to `#[serde_as]`, is serialized as `"_stage": null` and required on
//!    decode. In `7c7d831` the type had only `accounts` and `slots`.
//!
//! These mirror structs decode the `7c7d831` wire format (reusing the current
//! primitive types, which are otherwise serialization-compatible) and restore a
//! current `Block` by:
//! - rebuilding `execution_info` from `evm_input` (the same mapping `7c7d831`
//!   used in `From<TransactionExecution> for TransactionInput`), and
//! - rebuilding `ExecutionChanges<Complete>` from the stage-less mirror.
//!
//! The resulting `Block` is then converted to `BlockRocksdb` through the
//! existing `From<Block> for BlockRocksdb`, yielding exactly what a `7c7d831`
//! leader would have produced had it returned `BlockRocksdb` directly.

use std::collections::HashMap;

use hash_hasher::HashBuildHasher;
use serde_with::serde_as;

use crate::eth::executor::EvmExecutionResult;
use crate::eth::executor::EvmInput;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockHeader;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::Complete;
use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::EvmExecutionMetrics;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::ExecutionChanges;
use crate::eth::primitives::ExecutionInfo;
use crate::eth::primitives::ExecutionResult;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Log;
use crate::eth::primitives::MinedData;
use crate::eth::primitives::Signature;
use crate::eth::primitives::Signer;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotValue;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionInfo;
use crate::eth::primitives::TransactionMined;
use crate::eth::primitives::UnixTime;

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
    pub result: CompatEvmExecutionResult,
}

/// `7c7d831`-shaped `EvmExecutionResult`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CompatEvmExecutionResult {
    pub execution: CompatEvmExecution,
    pub metrics: EvmExecutionMetrics,
}

/// `7c7d831`-shaped `EvmExecution` (carries a stage-less `changes`).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CompatEvmExecution {
    pub block_timestamp: UnixTime,
    pub result: ExecutionResult,
    pub output: Bytes,
    pub logs: Vec<Log>,
    pub gas_used: Gas,
    pub changes: CompatExecutionChanges,
    pub deployed_contract_address: Option<Address>,
}

/// `7c7d831`-shaped `ExecutionChanges` (no `_stage` field).
#[serde_as]
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CompatExecutionChanges {
    pub accounts: HashMap<Address, ExecutionAccountChanges, HashBuildHasher>,
    #[serde_as(as = "Vec<(_, _)>")]
    pub slots: HashMap<(Address, SlotIndex), SlotValue, HashBuildHasher>,
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

        // Restore the staged `ExecutionChanges<Complete>` from the stage-less
        // mirror (the only difference is the `_stage: PhantomData` marker,
        // which is left at its default).
        let mut changes = ExecutionChanges::<Complete>::default();
        changes.accounts = execution.result.execution.changes.accounts;
        changes.slots = execution.result.execution.changes.slots;

        let restored_execution = TransactionExecution {
            info: execution.info,
            signature: execution.signature,
            execution_info,
            evm_input: execution.evm_input,
            result: EvmExecutionResult {
                execution: EvmExecution {
                    block_timestamp: execution.result.execution.block_timestamp,
                    result: execution.result.execution.result,
                    output: execution.result.execution.output,
                    logs: execution.result.execution.logs,
                    gas_used: execution.result.execution.gas_used,
                    changes,
                    deployed_contract_address: execution.result.execution.deployed_contract_address,
                },
                metrics: execution.result.metrics,
            },
        };

        TransactionMined {
            execution: restored_execution,
            mined_data,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::CompatBlock;
    use crate::eth::primitives::Address;
    use crate::eth::primitives::Block;
    use crate::eth::primitives::BlockNumber;
    use crate::eth::primitives::Hash;
    use crate::eth::primitives::Signer;
    use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;
    use crate::eth::storage::permanent::rocks::types::BlockRocksdb;

    /// Fixture captured from a real `7c7d831` leader response to
    /// `stratus_getBlockWithChanges` (trimmed to 1 transaction + 2 account / 2
    /// slot changes). The wire `result` is a JSON array `[block, changes]`.
    const FIXTURE_7C7D831: &str = include_str!("tests_fixtures/getblockwithchanges_7c7d831.json");

    /// The current `Block` type cannot decode a `7c7d831` block because its
    /// `TransactionExecution` requires an `execution_info` field that the
    /// legacy wire format does not carry. This guards the compat layer: if the
    /// regression is ever "fixed" by dropping the compat path, this assertion
    /// will keep failing until the leader is upgraded.
    #[test]
    fn current_block_cannot_decode_7c7d831_wire_format() {
        let err = serde_json::from_str::<(Block, BlockChangesRocksdb)>(FIXTURE_7C7D831);
        assert!(err.is_err(), "current Block should fail to decode 7c7d831 wire format, but it succeeded");
        let msg = err.unwrap_err().to_string();
        assert!(
            msg.contains("execution_info") || msg.contains("_stage"),
            "expected a 7c7d831-incompatibility error (execution_info or _stage), got: {msg}"
        );
    }

    /// The compat layer decodes the `7c7d831` wire format and rebuilds a
    /// current `Block` whose synthesized `execution_info` matches the
    /// `evm_input` it was derived from — the same mapping `7c7d831` itself
    /// used in `From<TransactionExecution> for TransactionInput`.
    #[test]
    fn compat_block_decodes_and_restores_execution_info() {
        let (compat_block, _changes): (CompatBlock, BlockChangesRocksdb) =
            serde_json::from_str(FIXTURE_7C7D831).expect("fixture must decode as (CompatBlock, BlockChangesRocksdb)");

        // ground truth from the fixture header.
        assert_eq!(compat_block.header.number, BlockNumber::from_str("0x8248dac").unwrap());
        assert_eq!(
            compat_block.header.hash,
            Hash::from_str("0xe097f046340bdb101eb97315249e8918ffd8cc25767b75b3e75b39e2c974621f").unwrap()
        );
        assert_eq!(compat_block.transactions.len(), 1);

        // convert to current Block and verify execution_info was rebuilt from evm_input.
        let block = Block::from(compat_block);
        assert_eq!(block.transactions.len(), 1);

        let tx = &block.transactions[0];
        let evm_input = &tx.evm_input;
        let info = &tx.execution_info;

        assert_eq!(info.chain_id, evm_input.chain_id);
        assert_eq!(info.nonce, evm_input.nonce.unwrap_or_default());
        assert_eq!(info.signer, Signer::Recovered(evm_input.from));
        assert_eq!(info.to, evm_input.to);
        assert_eq!(info.value, evm_input.value);
        assert_eq!(info.input, evm_input.data);
        assert_eq!(info.gas_limit, evm_input.gas_limit);
        assert_eq!(info.gas_price, evm_input.gas_price);

        // signer must be the recovered `from` address from the fixture.
        assert_eq!(
            info.signer,
            Signer::Recovered(Address::from_str("0xa7249d2c214c8b452fb1a600bd2d42c174b3c903").unwrap())
        );
    }

    /// The hotfix path `CompatBlock -> Block -> BlockRocksdb` must succeed and
    /// preserve the header and transaction count, producing exactly what the
    /// `fetch_block_with_changes` client now returns to the importer.
    #[test]
    fn compat_block_converts_to_block_rocksdb() {
        let (compat_block, _changes): (CompatBlock, BlockChangesRocksdb) = serde_json::from_str(FIXTURE_7C7D831).expect("fixture must decode");

        let expected_number = compat_block.header.number;
        let expected_hash = compat_block.header.hash;
        let expected_txs = compat_block.transactions.len();

        let block = Block::from(compat_block);
        let rocks_block = BlockRocksdb::from(block);

        assert_eq!(rocks_block.transactions.len(), expected_txs);
        assert_eq!(rocks_block.header.number, expected_number.into());
        assert_eq!(rocks_block.header.hash, expected_hash.into());
    }
}
