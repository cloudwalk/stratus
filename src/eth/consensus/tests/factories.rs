use rand::Rng;
use ethereum_types::{H256, U256};
use crate::eth::consensus::{BlockEntry, TransactionExecutionEntry, LogEntryData};
use crate::eth::consensus::log_entry::LogEntry;
use crate::eth::consensus::append_entry::Log;

pub fn create_mock_block_entry() -> BlockEntry {
    BlockEntry {
        number: rand::thread_rng().gen(),
        hash: H256::random().to_string(),
        parent_hash: H256::random().to_string(),
        nonce: H256::random().to_string(),
        uncle_hash: H256::random().to_string(),
        transactions_root: H256::random().to_string(),
        state_root: H256::random().to_string(),
        receipts_root: H256::random().to_string(),
        miner: H256::random().to_string(),
        difficulty: U256::from(rand::thread_rng().gen::<u64>()).to_string(),
        total_difficulty: U256::from(rand::thread_rng().gen::<u64>()).to_string(),
        extra_data: vec![rand::thread_rng().gen()],
        size: rand::thread_rng().gen(),
        gas_limit: U256::from(rand::thread_rng().gen::<u64>()).to_string(),
        gas_used: U256::from(rand::thread_rng().gen::<u64>()).to_string(),
        timestamp: rand::thread_rng().gen(),
        bloom: H256::random().to_string(),
        author: H256::random().to_string(),
        transaction_hashes: vec![H256::random().to_string()],
    }
}

pub fn create_mock_transaction_execution_entry() -> TransactionExecutionEntry {
    TransactionExecutionEntry {
        hash: H256::random().to_string(),
        nonce: rand::thread_rng().gen(),
        value: vec![rand::thread_rng().gen()],
        gas_price: vec![rand::thread_rng().gen()],
        input: vec![rand::thread_rng().gen()],
        v: rand::thread_rng().gen(),
        r: vec![rand::thread_rng().gen()],
        s: vec![rand::thread_rng().gen()],
        chain_id: rand::thread_rng().gen(),
        result: vec![rand::thread_rng().gen()],
        output: vec![rand::thread_rng().gen()],
        from: H256::random().to_string(),
        to: H256::random().to_string(),
        block_hash: H256::random().to_string(),
        block_number: rand::thread_rng().gen(),
        transaction_index: rand::thread_rng().gen(),
        logs: vec![Log {
            address: H256::random().to_string(),
            topics: vec![H256::random().to_string()],
            data: vec![rand::thread_rng().gen()],
            log_index: rand::thread_rng().gen(),
            removed: rand::thread_rng().gen(),
            transaction_log_index: rand::thread_rng().gen(),
        }],
        gas: vec![rand::thread_rng().gen()],
        receipt_cumulative_gas_used: vec![rand::thread_rng().gen()],
        receipt_gas_used: vec![rand::thread_rng().gen()],
        receipt_contract_address: vec![rand::thread_rng().gen()],
        receipt_status: rand::thread_rng().gen(),
        receipt_logs_bloom: vec![rand::thread_rng().gen()],
        receipt_effective_gas_price: vec![rand::thread_rng().gen()],
    }
}

pub fn create_mock_log_entry_data_block() -> LogEntryData {
    LogEntryData::BlockEntry(create_mock_block_entry())
}

pub fn create_mock_log_entry_data_transactions() -> LogEntryData {
    LogEntryData::TransactionExecutionEntries(vec![
        create_mock_transaction_execution_entry(),
        create_mock_transaction_execution_entry(),
    ])
}

pub fn create_mock_log_entry(index: u64, term: u64, data: LogEntryData) -> LogEntry {
    LogEntry {
        index,
        term,
        data,
    }
}
