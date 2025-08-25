use alloy_primitives::B256;
use alloy_rpc_types_eth::BlockTransactions;
use alloy_trie::root::ordered_trie_root;
use display_json::DebugAsJson;
use itertools::Itertools;

use super::ExternalBlock;
use super::Index;
use super::LogMined;
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
use crate::eth::primitives::TransactionMined;
use crate::eth::primitives::UnixTime;
use crate::ext::to_json_value;

#[derive(DebugAsJson, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
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
        Block::new(BlockNumber::ZERO, UnixTime::from(1702568764))
    }

    /// Calculates block size label by the number of transactions.
    pub fn label_size_by_transactions(&self) -> &'static str {
        match self.transactions.len() {
            0 => "0",
            1..=5 => "1-5",
            6..=10 => "6-10",
            11..=15 => "11-15",
            16..=20 => "16-20",
            _ => "20+",
        }
    }

    /// Calculates block size label by consumed gas.
    pub fn label_size_by_gas(&self) -> &'static str {
        match self.header.gas_used.as_u64() {
            0 => "0",
            1..=1_000_000 => "0-1M",
            1_000_001..=2_000_000 => "1M-2M",
            2_000_001..=3_000_000 => "2M-3M",
            3_000_001..=4_000_000 => "3M-4M",
            4_000_001..=5_000_000 => "4M-5M",
            5_000_001..=6_000_000 => "5M-6M",
            6_000_001..=7_000_000 => "6M-7M",
            7_000_001..=8_000_000 => "7M-8M",
            8_000_001..=9_000_000 => "8M-9M",
            9_000_001..=10_000_000 => "9M-10M",
            _ => "10M+",
        }
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

    fn mine_transaction(&mut self, tx: TransactionExecution, transaction_index: Index, log_index: &mut Index) -> TransactionMined {
        let mined_logs = Self::mine_logs(self, &tx, transaction_index, log_index);

        TransactionMined {
            input: tx.input,
            execution: tx.result.execution,
            transaction_index,
            block_number: self.header.number,
            block_hash: self.header.hash,
            logs: mined_logs,
        }
    }

    fn mine_logs(&mut self, tx: &TransactionExecution, transaction_index: Index, log_index: &mut Index) -> Vec<LogMined> {
        tx.result
            .execution
            .logs
            .iter()
            .map(|mined_log| {
                self.header.bloom.accrue_log(mined_log);
                let log = LogMined::mine_log(mined_log.clone(), self.number(), self.hash(), tx, *log_index, transaction_index);
                *log_index = *log_index + Index::ONE;
                log
            })
            .collect()
    }

    fn calculate_transaction_root(&mut self) {
        if !self.transactions.is_empty() {
            let transactions_hashes: Vec<B256> = self.transactions.iter().map(|x| x.input.hash).map(B256::from).collect();
            self.header.transactions_root = ordered_trie_root(&transactions_hashes).into();
        }
    }

    fn update_block_hash(&mut self) {
        for transaction in self.transactions.iter_mut() {
            transaction.block_hash = self.header.hash;
            for log in transaction.logs.iter_mut() {
                log.block_hash = self.header.hash;
            }
        }
    }

    pub fn apply_external(&mut self, external_block: &ExternalBlock) {
        self.header.hash = external_block.hash();
        self.header.timestamp = external_block.timestamp();
        for transaction in self.transactions.iter_mut() {
            assert!(transaction.execution.block_timestamp == self.header.timestamp);
            transaction.block_hash = external_block.hash();
            for log in transaction.logs.iter_mut() {
                log.block_hash = external_block.hash();
            }
        }
    }
}

impl From<PendingBlock> for Block {
    fn from(value: PendingBlock) -> Self {
        let mut block = Block::new(value.header.number, *value.header.timestamp);
        let txs: Vec<TransactionExecution> = value.transactions.into_values().collect();
        block.transactions.reserve(txs.len());
        block.header.size = Size::from(txs.len() as u64);

        let mut log_index = Index::ZERO;
        for (tx_idx, tx) in txs.into_iter().enumerate() {
            let transaction_index = Index::new(tx_idx as u64);
            let mined_transaction = Self::mine_transaction(&mut block, tx, transaction_index, &mut log_index);
            block.transactions.push(mined_transaction);
        }

        Self::calculate_transaction_root(&mut block);
        Self::update_block_hash(&mut block);

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
        let transaction_hashes: Vec<B256> = block.transactions.into_iter().map(|x| x.input.hash).map(B256::from).collect();

        Self {
            transactions: BlockTransactions::Hashes(transaction_hashes),
            ..alloy_block
        }
    }
}
