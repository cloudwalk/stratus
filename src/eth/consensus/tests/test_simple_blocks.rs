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
use crate::eth::consensus::TransactionExecutionEntry;

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
}
