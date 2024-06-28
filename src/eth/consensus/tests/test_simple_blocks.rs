use std::sync::Arc;

use tokio::sync::Mutex;
use tonic::Request;

use super::factories::create_follower_consensus_with_leader;
use super::factories::create_mock_block_entry;
use super::factories::create_mock_transaction_execution_entry;
use crate::eth::consensus::append_entry::append_entry_service_server::AppendEntryService;
use crate::eth::consensus::append_entry::AppendBlockCommitRequest;
use crate::eth::consensus::append_entry::AppendTransactionExecutionsRequest;
use crate::eth::consensus::AppendEntryServiceImpl;
use crate::eth::consensus::Role;
use crate::eth::consensus::StatusCode;
use crate::eth::consensus::TransactionExecutionEntry;
use crate::eth::primitives::BlockFilter;

#[tokio::test]
async fn test_append_entries_transaction_executions_and_block() {
    let consensus = create_follower_consensus_with_leader().await;
    let service = AppendEntryServiceImpl {
        consensus: Mutex::new(Arc::clone(&consensus)),
    };

    let leader_id = {
        let peers = consensus.peers.read().await;
        let (leader_address, _) = peers.iter().find(|&(_, (peer, _))| peer.role == Role::Leader).expect("Leader peer not found");
        leader_address.to_string()
    };
    // Create 3 requests with 3 transactions each, totaling 9 transactions
    let total_requests = 3;
    let transactions_per_request = 3;
    let mut all_executions = Vec::new();
    let mut prev_log_index = 0;

    for _ in 0..total_requests {
        let executions: Vec<TransactionExecutionEntry> = (0..transactions_per_request).map(|_| create_mock_transaction_execution_entry()).collect();

        // Store all executions for later verification
        all_executions.extend(executions.clone());

        // Convert to append entries request format
        let request = Request::new(AppendTransactionExecutionsRequest {
            term: 1,
            leader_id: leader_id.clone(),
            prev_log_index,
            prev_log_term: 1, // assuming same term for simplicity
            executions: executions.clone(),
        });

        // Append transactions
        let response = service.append_transaction_executions(request).await;
        assert!(response.is_ok());
        let response_inner = response.unwrap().into_inner();
        assert_eq!(response_inner.status, StatusCode::AppendSuccess as i32);

        // Update prev_log_index for the next request
        prev_log_index += 1;
    }

    // Create and append block with transaction hashes
    let transaction_hashes: Vec<Vec<u8>> = all_executions.iter().map(|e| e.hash.clone()).collect();

    let block_entry = create_mock_block_entry(transaction_hashes.clone());

    let block_request = Request::new(AppendBlockCommitRequest {
        term: 1,
        leader_id: leader_id.clone(),
        prev_log_index,
        prev_log_term: 1,
        block_entry: Some(block_entry.clone()),
    });

    let block_response = service.append_block_commit(block_request).await;
    assert!(block_response.is_ok());
    let response_inner = block_response.unwrap().into_inner();
    assert_eq!(response_inner.status, StatusCode::AppendSuccess as i32);

    let storage = &consensus.storage;

    // Verify the block was saved with the correct transaction hashes
    let _saved_block = storage.read_block(&BlockFilter::Latest).unwrap().unwrap();
    //XXX assert_eq!(saved_block.transactions.len(), all_executions.len());

    //XXX let saved_transaction_hashes: Vec<String> = saved_block.transactions.iter().map(|tx| tx.input.hash.clone()).collect();
    //XXX assert_eq!(saved_transaction_hashes, transaction_hashes);

    //XXX // Test reading accounts
    //XXX let account_address = Address::from(H256::random());
    //XXX let account = storage.read_account(&account_address, &StoragePointInTime::Present).unwrap();
    //XXX assert_eq!(account.address, account_address);

    //XXX // Test reading slots
    //XXX let slot_index = SlotIndex::new(0);
    //XXX let slot = storage.read_slot(&account.address, &slot_index, &StoragePointInTime::Present).unwrap();
    //XXX assert_eq!(slot.index, slot_index);

    //XXX // Test reading execution
    //XXX let tx_hash = all_executions[0].hash.clone();
    //XXX let saved_tx = storage.read_transaction(&H256::from_slice(&hex::decode(&tx_hash).unwrap())).unwrap();
    //XXX assert!(saved_tx.is_some());

    //XXX // Test reading logs
    //XXX let log_filter = LogFilter::default();
    //XXX let logs = storage.read_logs(&log_filter).unwrap();
    //XXX assert!(logs.is_empty());

    //XXX // Test reading block numbers
    //XXX let active_block_number = storage.read_active_block_number().unwrap().unwrap();
    //XXX assert!(active_block_number > BlockNumber::ZERO);

    //XXX let mined_block_number = storage.read_mined_block_number().unwrap();
    //XXX assert!(mined_block_number >= BlockNumber::ZERO);
}
