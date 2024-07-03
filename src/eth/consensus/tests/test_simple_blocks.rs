use std::sync::Arc;

use ethereum_types::H160;
use ethereum_types::H256;
use hex_literal::hex;
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
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StoragePointInTime;

#[tokio::test]
async fn test_append_entries_transaction_executions_and_block() {
    let consensus = create_follower_consensus_with_leader(None).await;
    let service = AppendEntryServiceImpl {
        consensus: Mutex::new(Arc::clone(&consensus)),
    };

    let leader_id = {
        let peers = consensus.peers.read().await;
        let (leader_address, _) = peers.iter().find(|&(_, (peer, _))| peer.role == Role::Leader).expect("Leader peer not found");
        leader_address.to_string()
    };

    let total_requests = 3;
    let transactions_per_request = 3;
    let mut all_executions = Vec::new();
    let mut prev_log_index = 0;

    // Provided deterministic hashes
    let provided_hashes = vec![
        Hash::new(hex!("60581af145126156b499ac08b42a37a46d9f3b5684a723152d415c2cb9e797f9")),
        Hash::new(hex!("bceda89e22137dbc65b22313987e2879183a572eb62f4dd63bcacd334d78fa17")),
        Hash::new(hex!("4c5a9cd79146318ddf66a3eaa3bb01b036616ab745e97f46bd9965e39f8ffaf1")),
        Hash::new(hex!("69f1e3f3074d3d9711edbf5aea3ea3f4b955a8115a8c46695087f2f2b2c6f343")),
        Hash::new(hex!("0fbb7855bd2c7add38d3bf2e0e64e000e553c066ef984d0a93c35b4871607d6d")),
        Hash::new(hex!("cec569fe883d36a17addacfc5c5b21fdccad38a7eec862cbe02d40e20cb602eb")),
        Hash::new(hex!("66f7f14c633fe76d08d690d25b9fda8094b64a9e5c6b0dba3a410eb5181ccb62")),
        Hash::new(hex!("6e8fed1ab67fa3955c1c32e8ab568d1ce6e282ccea06844029b954d2586cb979")),
        Hash::new(hex!("9c4babff06b9ea539e4221102069e6fa3775877fa8fc6316e5df6bfe909d56e2")),
    ];

    let mut hash_index = 0;
    for _ in 0..total_requests {
        let executions: Vec<TransactionExecutionEntry> = (0..transactions_per_request)
            .map(|_| {
                let entry = create_mock_transaction_execution_entry(Some(provided_hashes[hash_index]));
                hash_index += 1;
                entry
            })
            .collect();

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

    let block_entry = create_mock_block_entry(
        transaction_hashes.clone(),
        Some(Hash::new(hex!("f07543db1bff9fa1a639578e2d90949a56822c108c63ca1b2f2a55deb2d562fc"))),
    );

    let block_request = Request::new(AppendBlockCommitRequest {
        term: 1,
        leader_id: leader_id.clone(),
        prev_log_index,
        prev_log_term: 1,
        block_entry: Some(block_entry.clone()),
    });

    let block_response = service.append_block_commit(block_request).await;
    assert!(block_response.is_ok(), "{}", format!("{:?}", block_response));
    let response_inner = block_response.unwrap().into_inner();
    assert_eq!(response_inner.status, StatusCode::AppendSuccess as i32);

    let storage = &consensus.storage;

    // Verify the block was saved with the correct transaction hashes
    let saved_block = storage.read_block(&BlockFilter::Latest).unwrap().unwrap();
    assert_eq!(saved_block.transactions.len(), 9);

    let saved_transaction_hashes: Vec<Hash> = saved_block.transactions.iter().map(|tx| tx.input.hash).collect();
    let expected_transaction_hashes: Vec<Hash> = transaction_hashes.iter().map(|hash| Hash::new_from_h256(H256::from_slice(hash))).collect();

    assert_eq!(saved_transaction_hashes, expected_transaction_hashes);

    // Test reading transaction execution
    let tx_hash = all_executions[0].hash.clone();
    let saved_tx = storage.read_transaction(&Hash::new_from_h256(H256::from_slice(&tx_hash))).unwrap();
    assert!(saved_tx.is_some());

    // Test reading logs
    let log_filter = LogFilter::default();
    let logs = storage.read_logs(&log_filter).unwrap();
    assert!(logs.is_empty());

    // Test reading execution
    let tx_hash = all_executions[0].hash.clone();
    let saved_tx = storage.read_transaction(&Hash::new_from_h256(H256::from_slice(&tx_hash))).unwrap();
    assert!(saved_tx.is_some());

    // Test reading logs
    let log_filter = LogFilter::default();
    let logs = storage.read_logs(&log_filter).unwrap();
    assert!(logs.is_empty());

    // Test reading block numbers
    let active_block_number = storage.read_active_block_number().unwrap().unwrap();
    assert!(active_block_number > BlockNumber::ZERO);

    let mined_block_number = storage.read_mined_block_number().unwrap();
    assert!(mined_block_number >= BlockNumber::ZERO);

    for execution in all_executions.iter() {
        let _account = storage
            .read_account(&Address::new_from_h160(H160::from_slice(&execution.from)), &StoragePointInTime::Mined)
            .unwrap();
        //TODO test account details
    }

    for execution in all_executions.iter() {
        let _slot = storage
            .read_slot(
                &Address::new_from_h160(H160::from_slice(&execution.from)),
                &SlotIndex::ZERO,
                &StoragePointInTime::Mined,
            )
            .unwrap();
        //TODO test slot details
    }

    for execution in all_executions.iter() {
        let _slots = storage
            .read_all_slots(&Address::new_from_h160(H160::from_slice(&execution.from)), &StoragePointInTime::Mined)
            .unwrap();
        //TODO test slots details
    }

    // generates a second block
    let block_entry = create_mock_block_entry(vec![], None);

    let block_request = Request::new(AppendBlockCommitRequest {
        term: 1,
        leader_id: leader_id.clone(),
        prev_log_index,
        prev_log_term: 1,
        block_entry: Some(block_entry.clone()),
    });

    let block_response = service.append_block_commit(block_request).await;
    assert!(block_response.is_ok(), "{}", format!("{:?}", block_response));
    let response_inner = block_response.unwrap().into_inner();
    assert_eq!(response_inner.status, StatusCode::AppendSuccess as i32);

    let saved_block = storage.read_block(&BlockFilter::Latest).unwrap().unwrap();
    assert_eq!(saved_block.transactions.len(), 0);

    // generates a third block
    let provided_hashes = [
        Hash::new(hex!("056bd43a37cafa7321985c7b4c5f228db2f2f9e4e20307b88e4427a7ee8d273c")),
        Hash::new(hex!("0891f0534d157cbf567a6b5e4d3894d169af57f529ed31448045dd1314fe0bb1")),
        Hash::new(hex!("22caaef98424a1a1f95d3604ecd35d99c8f82014b69be201e0d31fd44a512b24")),
    ];

    let mut hash_index = 0;
    let executions: Vec<TransactionExecutionEntry> = (0..transactions_per_request)
        .map(|_| {
            let entry = create_mock_transaction_execution_entry(Some(provided_hashes[hash_index]));
            hash_index += 1;
            entry
        })
        .collect();

    let request = Request::new(AppendTransactionExecutionsRequest {
        term: 1,
        leader_id: leader_id.clone(),
        prev_log_index,
        prev_log_term: 1, // assuming same term for simplicity
        executions: executions.clone(),
    });

    let response = service.append_transaction_executions(request).await;
    assert!(response.is_ok());
    let response_inner = response.unwrap().into_inner();
    assert_eq!(response_inner.status, StatusCode::AppendSuccess as i32);

    let transaction_hashes: Vec<Vec<u8>> = executions.iter().map(|e| e.hash.clone()).collect();

    let block_entry = create_mock_block_entry(
        transaction_hashes,
        Some(Hash::new(hex!("1ba6cc210ec982f09290338c2bd8b501bf2ca40ff153c11dd1b10a57128b5b64"))),
    );

    let block_request = Request::new(AppendBlockCommitRequest {
        term: 1,
        leader_id: leader_id.clone(),
        prev_log_index,
        prev_log_term: 1,
        block_entry: Some(block_entry.clone()),
    });

    let block_response = service.append_block_commit(block_request).await;
    assert!(block_response.is_ok(), "{}", format!("{:?}", block_response));
    let response_inner = block_response.unwrap().into_inner();
    assert_eq!(response_inner.status, StatusCode::AppendSuccess as i32);

    let saved_block = storage.read_block(&BlockFilter::Latest).unwrap().unwrap();
    assert_eq!(saved_block.transactions.len(), 3);
}
