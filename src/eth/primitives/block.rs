use alloy_primitives::B256;
use alloy_primitives::keccak256;
use alloy_rpc_types_eth::BlockTransactions;
use alloy_trie::root::ordered_trie_root;
use display_json::DebugAsJson;
use itertools::Itertools;

use super::ExternalBlock;
use super::Index;
use super::PendingBlock;
use super::Size;
use super::TransactionExecution;
use crate::alias::AlloyBlockAlloyTransaction;
use crate::alias::AlloyBlockB256;
use crate::alias::AlloyTransaction;
use crate::alias::JsonValue;
use crate::eth::primitives::BlockHeader;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogMessage;
use crate::eth::primitives::TransactionMined;
use crate::eth::primitives::UnixTime;
use crate::ext::to_json_value;

#[derive(DebugAsJson, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<TransactionMined>,
}

impl Block {
    /// Creates a new block with the given number and timestamp.
    pub fn new(number: BlockNumber, timestamp: UnixTime) -> Self {
        Self {
            header: BlockHeader::new(number, timestamp),
            transactions: Vec::new(),
        }
    }

    /// Constructs an empty genesis block.
    pub fn genesis() -> Block {
        let mut block = Block::new(BlockNumber::ZERO, UnixTime::from(1702568764));
        block.header.hash = block.calculate_hash_v1();
        block
    }

    /// Serializes itself to JSON-RPC block format with full transactions included.
    pub fn to_json_rpc_with_full_transactions(self) -> JsonValue {
        let alloy_block: AlloyBlockAlloyTransaction = self.into();
        to_json_value(alloy_block)
    }

    /// Serializes itself to JSON-RPC block format with only transactions hashes included.
    pub fn to_json_rpc_with_transactions_hashes(self) -> JsonValue {
        let alloy_block: AlloyBlockB256 = self.into();
        to_json_value(alloy_block)
    }

    /// Returns the block number.
    pub fn number(&self) -> BlockNumber {
        self.header.number
    }

    /// Returns the block hash.
    pub fn hash(&self) -> Hash {
        self.header.hash
    }

    pub fn create_log_messages(&self) -> Vec<LogMessage> {
        let mut log_messages = vec![];
        for (transaction_index, tx) in self.transactions.iter().enumerate() {
            for (idx, log) in tx.logs().iter().enumerate() {
                log_messages.push(LogMessage {
                    log: log.clone(),
                    transaction_hash: tx.info.hash,
                    transaction_index: (transaction_index as u64).into(),
                    block_hash: self.hash(),
                    block_number: self.number(),
                    index: tx.mined_data.first_log_index + Index(idx as u64),
                });
            }
        }
        log_messages
    }

    fn calculate_transaction_root(&mut self) {
        if !self.transactions.is_empty() {
            let transactions_hashes: Vec<B256> = self.transactions.iter().map(|x| x.info.hash).map(B256::from).collect();
            self.header.transactions_root = ordered_trie_root(&transactions_hashes).into();
        }
    }

    pub fn calculate_hash_v1(&self) -> Hash {
        self.number().hash()
    }

    pub fn calculate_hash_v2(&self) -> Hash {
        let mut input = [0_u8; 80];
        input[0..8].copy_from_slice(&self.number().as_u64().to_be_bytes());
        input[8..16].copy_from_slice(&self.header.timestamp.to_be_bytes());
        input[16..48].copy_from_slice(self.header.transactions_root.as_ref());
        input[48..80].copy_from_slice(self.header.parent_hash.as_ref());
        keccak256(input).into()
    }

    pub fn calculate_hash_default(&self) -> Hash {
        self.calculate_hash_v2()
    }

    pub fn apply_hash(&mut self, hash: Hash) {
        self.header.hash = hash;
        for transaction in self.transactions.iter_mut() {
            transaction.mined_data.block_hash = hash;
        }
    }

    pub fn apply_default_hash(&mut self) {
        let hash = self.calculate_hash_default();
        self.apply_hash(hash);
    }

    pub fn apply_external(&mut self, external_block: &ExternalBlock) {
        assert!(*self.header.timestamp == external_block.header.timestamp);
        // The reexecutor trusts the imported parent hash stored in the pending block.

        let external_hash = external_block.hash();
        let default_hash = self.calculate_hash_default();
        if external_hash != default_hash {
            // TODO: Remove the V1 hash arm after every node has been upgraded.
            let v1_hash = self.calculate_hash_v1();
            assert!(
                external_hash == v1_hash,
                "invalid external block hash: imported={external_hash} default={default_hash} v1={v1_hash}"
            );
        }

        self.apply_hash(external_hash);
    }
}

impl From<PendingBlock> for Block {
    fn from(value: PendingBlock) -> Self {
        let mut block = Block::new(value.header.number, *value.header.timestamp);
        block.header.parent_hash = value.header.parent_hash.unwrap_or(Hash::ZERO);
        let txs: Vec<TransactionExecution> = value.transactions.into_values().collect();
        block.transactions.reserve(txs.len());
        block.header.size = Size::from(txs.len() as u64);

        let mut log_index = Index::ZERO;
        for (tx_idx, execution) in txs.into_iter().enumerate() {
            let log_count = execution.result.execution.logs.len() as u64;
            let transaction_mined = TransactionMined::from_execution(execution, block.hash(), (tx_idx as u64).into(), log_index);
            block.header.gas_used += transaction_mined.execution.result.execution.gas_used;
            block.transactions.push(transaction_mined);
            log_index += Index(log_count);
        }

        Self::calculate_transaction_root(&mut block);

        block
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<Block> for AlloyBlockAlloyTransaction {
    fn from(block: Block) -> Self {
        let alloy_block: AlloyBlockAlloyTransaction = block.header.into();
        let transactions: Vec<AlloyTransaction> = block.transactions.into_iter().map_into().collect();

        Self {
            transactions: BlockTransactions::Full(transactions),
            ..alloy_block
        }
    }
}

impl From<Block> for AlloyBlockB256 {
    fn from(block: Block) -> Self {
        let alloy_block: AlloyBlockB256 = block.header.into();
        let transaction_hashes: Vec<B256> = block.transactions.into_iter().map(|x| x.info.hash).map(B256::from).collect();

        Self {
            transactions: BlockTransactions::Hashes(transaction_hashes),
            ..alloy_block
        }
    }
}

#[cfg(test)]
mod tests {
    use fake::Fake;
    use fake::Faker;

    use super::*;

    fn block_with_v2_hash() -> Block {
        let mut block = Block::new(BlockNumber::ONE, UnixTime::from(1234567890));
        block.header.transactions_root = Hash::new([1; 32]);
        block.header.parent_hash = Hash::new([2; 32]);
        block.apply_default_hash();
        block
    }

    fn external_block(block: &Block, hash: Hash) -> ExternalBlock {
        let mut external: ExternalBlock = Faker.fake();
        external.0.header.inner.number = block.number().as_u64();
        external.0.header.inner.timestamp = *block.header.timestamp;
        external.0.header.inner.parent_hash = block.header.parent_hash.into();
        external.0.header.hash = hash.into();
        external.0.transactions = BlockTransactions::Full(Vec::new());
        external
    }

    #[test]
    fn v2_hash_depends_on_finalized_block_fields() {
        let block = block_with_v2_hash();

        let mut changed = block.clone();
        changed.header.number = BlockNumber::from(2_u64);
        changed.apply_default_hash();
        assert_ne!(block.hash(), changed.hash());

        changed = block.clone();
        changed.header.timestamp = UnixTime::from(*changed.header.timestamp + 1);
        changed.apply_default_hash();
        assert_ne!(block.hash(), changed.hash());

        changed = block.clone();
        changed.header.transactions_root = Hash::new([3; 32]);
        changed.apply_default_hash();
        assert_ne!(block.hash(), changed.hash());

        changed = block.clone();
        changed.header.parent_hash = Hash::new([4; 32]);
        changed.apply_default_hash();
        assert_ne!(block.hash(), changed.hash());
    }

    #[test]
    fn external_hash_must_be_v2_or_v1() {
        let block = block_with_v2_hash();

        let mut v2_block = block.clone();
        v2_block.header.hash = Hash::ZERO;
        v2_block.apply_external(&external_block(&block, block.hash()));
        assert_eq!(v2_block.hash(), block.hash());

        let v1_hash = block.calculate_hash_v1();
        let mut v1_block = block.clone();
        v1_block.header.hash = Hash::ZERO;
        v1_block.apply_external(&external_block(&block, v1_hash));
        assert_eq!(v1_block.hash(), v1_hash);

        let mut invalid_block = block.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            invalid_block.apply_external(&external_block(&block, Hash::ZERO));
        }));
        assert!(result.is_err());
    }

    #[test]
    fn genesis_uses_legacy_hash() {
        let genesis = Block::genesis();
        assert_eq!(genesis.hash(), BlockNumber::ZERO.hash());
    }
}
