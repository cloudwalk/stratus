use core::sync::atomic::Ordering;
use std::sync::Arc;

use tokio::sync::Mutex;
use tonic::Request;
use tonic::Response;
use tonic::Status;

use super::append_entry::AppendBlockCommitRequest;
use super::append_entry::AppendBlockCommitResponse;
use super::append_entry::AppendTransactionExecutionsRequest;
use super::append_entry::AppendTransactionExecutionsResponse;
use super::append_entry::RequestVoteRequest;
use super::append_entry::RequestVoteResponse;
use super::append_entry::StatusCode;
use crate::eth::consensus::AppendEntryService;
use crate::eth::consensus::LogEntryData;
use crate::eth::consensus::PeerAddress;
use crate::eth::consensus::Role;
use crate::eth::primitives::Block;
use crate::eth::Consensus;
#[cfg(feature = "metrics")]
use crate::infra::metrics;

#[cfg(feature = "metrics")]
mod label {
    pub(super) const APPEND_TRANSACTION_EXECUTIONS: &str = "append_transaction_executions";
    pub(super) const APPEND_BLOCK_COMMIT: &str = "append_block_commit";
    pub(super) const REQUEST_VOTE: &str = "request_vote";
}

pub struct AppendEntryServiceImpl {
    pub consensus: Mutex<Arc<Consensus>>,
}

#[tonic::async_trait]
impl AppendEntryService for AppendEntryServiceImpl {
    async fn append_transaction_executions(
        &self,
        request: Request<AppendTransactionExecutionsRequest>,
    ) -> Result<Response<AppendTransactionExecutionsResponse>, Status> {
        #[cfg(feature = "metrics")]
        let start = std::time::Instant::now();

        let consensus = self.consensus.lock().await;
        let current_term = consensus.current_term.load(Ordering::SeqCst);
        let request_inner = request.into_inner();

        if consensus.is_leader() {
            tracing::error!(sender = request_inner.leader_id, "append_transaction_executions called on leader node");
            return Err(Status::new(
                (StatusCode::NotLeader as i32).into(),
                "append_transaction_executions called on leader node".to_string(),
            ));
        }

        // Return error if request term < current term
        if request_inner.term < current_term {
            let error_message = format!("Request term {} is less than current term {}", request_inner.term, current_term);
            tracing::error!(request_term = request_inner.term, current_term = current_term, "{}", &error_message);
            return Err(Status::new((StatusCode::TermMismatch as i32).into(), error_message));
        }

        let executions = request_inner.executions;
        let index = request_inner.prev_log_index + 1;
        let term = request_inner.prev_log_term;
        let data = LogEntryData::TransactionExecutionEntries(executions.clone());

        #[cfg(feature = "rocks")]
        if let Err(e) = consensus.log_entries_storage.save_log_entry(index, term, data, "transaction") {
            tracing::error!("Failed to save log entry: {:?}", e);
            return Err(Status::internal("Failed to save log entry"));
        }

        //TODO send the executions to the Storage
        tracing::info!(executions = executions.len(), "appending executions");

        if let Ok(leader_peer_address) = PeerAddress::from_string(request_inner.leader_id) {
            consensus.update_leader(leader_peer_address).await;
        }
        consensus.reset_heartbeat_signal.notify_waiters();
        //TODO change cached index from consensus after the storage is implemented

        #[cfg(feature = "metrics")]
        metrics::inc_consensus_grpc_requests_finished(start.elapsed(), label::APPEND_TRANSACTION_EXECUTIONS);

        Ok(Response::new(AppendTransactionExecutionsResponse {
            status: StatusCode::AppendSuccess as i32,
            message: "transaction Executions appended successfully".into(),
            last_committed_block_number: 0,
        }))
    }

    async fn append_block_commit(&self, request: Request<AppendBlockCommitRequest>) -> Result<Response<AppendBlockCommitResponse>, Status> {
        #[cfg(feature = "metrics")]
        let start = std::time::Instant::now();

        let consensus = self.consensus.lock().await;
        let current_term = consensus.current_term.load(Ordering::SeqCst);
        let request_inner = request.into_inner();

        if consensus.is_leader() {
            tracing::error!(sender = request_inner.leader_id, "append_block_commit called on leader node");
            return Err(Status::new(
                (StatusCode::NotLeader as i32).into(),
                "append_block_commit called on leader node".to_string(),
            ));
        }

        // Return error if request term < current term
        if request_inner.term < current_term {
            let error_message = format!("Request term {} is less than current term {}", request_inner.term, current_term);
            tracing::error!(request_term = request_inner.term, current_term = current_term, "{}", &error_message);
            return Err(Status::new((StatusCode::TermMismatch as i32).into(), error_message));
        }

        let Some(block_entry) = request_inner.block_entry else {
            return Err(Status::invalid_argument("empty block entry"));
        };

        tracing::info!(number = block_entry.number, "appending new block");

        let index = request_inner.prev_log_index + 1;
        let term = request_inner.prev_log_term;
        let data = LogEntryData::BlockEntry(block_entry.clone());

        #[cfg(feature = "rocks")]
        if let Err(e) = consensus.log_entries_storage.save_log_entry(index, term, data, "block") {
            tracing::error!("Failed to save log entry: {:?}", e);
            return Err(Status::internal("Failed to save log entry"));
        }

        let last_last_arrived_block_number = consensus.last_arrived_block_number.load(Ordering::SeqCst);

        //TODO FIXME move this code back when we have propagation: let Some(diff) = last_last_arrived_block_number.checked_sub(block_entry.number) else {
        //TODO FIXME move this code back when we have propagation:      tracing::error!(
        //TODO FIXME move this code back when we have propagation:          "leader is behind follower: arrived_block: {}, block_entry: {}",
        //TODO FIXME move this code back when we have propagation:          last_last_arrived_block_number,
        //TODO FIXME move this code back when we have propagation:          block_entry.number
        //TODO FIXME move this code back when we have propagation:      );
        //TODO FIXME move this code back when we have propagation:      return Err(Status::new(
        //TODO FIXME move this code back when we have propagation:          (StatusCode::EntryAlreadyExists as i32).into(),
        //TODO FIXME move this code back when we have propagation:          "leader is behind follower and should step down".to_string(),
        //TODO FIXME move this code back when we have propagation:      ));
        //TODO FIXME move this code back when we have propagation: };
        //TODO FIXME move this code back when we have propagation: #[cfg(feature = "metrics")]
        //TODO FIXME move this code back when we have propagation: metrics::set_append_entries_block_number_diff(diff);

        //TODO send the executions to the Storage
        let block_result = Block::from_append_entry_block(block_entry.clone());
        match block_result {
            Ok(block) => match consensus.storage.save_block(block) {
                Ok(_) => {
                    tracing::info!(block_number = block_entry.number, "block saved successfully");
                }
                Err(err) => {
                    tracing::error!("failed to save block: {:?}", err);
                    return Err(Status::internal("failed to save block"));
                }
            },
            Err(err) => {
                tracing::error!("failed to parse block: {:?}", err);
                return Err(Status::internal("failed to save block"));
            }
        }

        if let Ok(leader_peer_address) = PeerAddress::from_string(request_inner.leader_id) {
            consensus.update_leader(leader_peer_address).await;
        }
        consensus.reset_heartbeat_signal.notify_waiters();
        consensus.last_arrived_block_number.store(block_entry.number, Ordering::SeqCst);

        tracing::info!(
            last_last_arrived_block_number = last_last_arrived_block_number,
            new_last_arrived_block_number = consensus.last_arrived_block_number.load(Ordering::SeqCst),
            "last arrived block number set",
        );

        #[cfg(feature = "metrics")]
        metrics::inc_consensus_grpc_requests_finished(start.elapsed(), label::APPEND_BLOCK_COMMIT);

        Ok(Response::new(AppendBlockCommitResponse {
            status: StatusCode::AppendSuccess as i32,
            message: "Block Commit appended successfully".into(),
            last_committed_block_number: consensus.last_arrived_block_number.load(Ordering::SeqCst),
        }))
    }

    async fn request_vote(&self, request: Request<RequestVoteRequest>) -> Result<Response<RequestVoteResponse>, Status> {
        #[cfg(feature = "metrics")]
        let start = std::time::Instant::now();

        let request = request.into_inner();
        let consensus = self.consensus.lock().await;
        let current_term = consensus.current_term.load(Ordering::SeqCst);

        if request.term <= current_term {
            tracing::info!(
                vote_granted = false,
                current_term = current_term,
                request_term = request.term,
                "requestvote received with stale term on election"
            );
            return Ok(Response::new(RequestVoteResponse {
                term: current_term,
                vote_granted: false,
                message: format!("stale term: current_term {}, request_term {}", current_term, request.term),
            }));
        }

        let candidate_address = PeerAddress::from_string(request.candidate_id.clone()).unwrap(); //XXX FIXME replace with rpc error

        if request.last_log_index >= consensus.last_arrived_block_number.load(Ordering::SeqCst) {
            consensus.current_term.store(request.term, Ordering::SeqCst);
            consensus.set_role(Role::Follower);
            consensus.reset_heartbeat_signal.notify_waiters(); // reset the heartbeat signal to avoid election timeout just after voting

            let mut voted_for = consensus.voted_for.lock().await;
            *voted_for = Some(candidate_address.clone());

            tracing::info!(vote_granted = true, current_term = current_term, request_term = request.term, candidate_address = %candidate_address, "voted for candidate on election");
            return Ok(Response::new(RequestVoteResponse {
                term: request.term,
                vote_granted: true,
                message: "success".to_string(),
            }));
        }

        #[cfg(feature = "metrics")]
        metrics::inc_consensus_grpc_requests_finished(start.elapsed(), label::REQUEST_VOTE);

        Ok(Response::new(RequestVoteResponse {
            term: request.term,
            vote_granted: false,
            message: format!(
                "index is bellow expectation: last_log_index {}, last_arrived_block_number {}",
                request.last_log_index,
                consensus.last_arrived_block_number.load(Ordering::SeqCst)
            ),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eth::consensus::tests::factories::*;
    use crate::eth::consensus::BlockEntry;

    #[tokio::test]
    async fn test_append_transaction_executions_insert() {
        let consensus = create_mock_consensus().await;
        let service = AppendEntryServiceImpl {
            consensus: Mutex::new(Arc::clone(&consensus)),
        };

        consensus.set_role(Role::Follower);

        let executions = vec![create_mock_transaction_execution_entry()];

        let request = Request::new(AppendTransactionExecutionsRequest {
            term: 1,
            leader_id: "leader_id".to_string(),
            prev_log_index: 0,
            prev_log_term: 0,
            executions: executions.clone(),
        });

        let response = service.append_transaction_executions(request).await;
        assert!(response.is_ok());

        // Check if the log entry was inserted correctly
        let log_entries_storage = &consensus.log_entries_storage;
        let last_index = log_entries_storage.get_last_index().unwrap();
        let saved_entry = log_entries_storage.get_entry(last_index).unwrap().unwrap();

        if let LogEntryData::TransactionExecutionEntries(saved_executions) = saved_entry.data {
            assert_eq!(saved_executions, executions);
        } else {
            panic!("Expected transaction execution entries in the log entry");
        }
    }

    #[tokio::test]
    async fn test_append_transaction_executions_not_leader() {
        let consensus = create_leader_consensus().await;
        let service = AppendEntryServiceImpl {
            consensus: Mutex::new(Arc::clone(&consensus)),
        };

        // Simulate the node as not a leader
        consensus.set_role(Role::Follower);

        let request = Request::new(AppendTransactionExecutionsRequest {
            term: 1,
            leader_id: "leader_id".to_string(),
            prev_log_index: 0,
            prev_log_term: 0,
            executions: vec![],
        });

        let response = service.append_transaction_executions(request).await;
        assert!(response.is_ok());

        let response = response.unwrap().into_inner();
        assert_eq!(response.status, StatusCode::AppendSuccess as i32);
        assert_eq!(response.message, "transaction Executions appended successfully");
        assert_eq!(response.last_committed_block_number, 0);
    }

    #[tokio::test]
    async fn test_append_transaction_executions_leader() {
        let consensus = create_leader_consensus().await;
        let service = AppendEntryServiceImpl {
            consensus: Mutex::new(Arc::clone(&consensus)),
        };

        // Simulate the node as a leader
        consensus.set_role(Role::Leader);

        let request = Request::new(AppendTransactionExecutionsRequest {
            term: 1,
            leader_id: "leader_id".to_string(),
            prev_log_index: 0,
            prev_log_term: 0,
            executions: vec![],
        });

        let response = service.append_transaction_executions(request).await;
        assert!(response.is_err());

        let status = response.unwrap_err();
        assert_eq!(status.code(), tonic::Code::Unknown);
        assert_eq!(status.message(), "append_transaction_executions called on leader node");
    }

    #[tokio::test]
    async fn test_append_block_commit_not_leader() {
        let consensus = create_follower_consensus_with_leader().await;
        let service = AppendEntryServiceImpl {
            consensus: Mutex::new(Arc::clone(&consensus)),
        };

        let leader_id = {
            let peers = consensus.peers.read().await;
            let (leader_address, _) = peers.iter().find(|&(_, (peer, _))| peer.role == Role::Leader).expect("Leader peer not found");
            leader_address.to_string()
        };

        let request = Request::new(AppendBlockCommitRequest {
            term: 1,
            leader_id,
            prev_log_index: 0,
            prev_log_term: 0,
            block_entry: Some(create_mock_block_entry(vec![])),
        });

        let response = service.append_block_commit(request).await;

        assert!(response.is_ok());

        let response = response.unwrap().into_inner();
        assert_eq!(response.status, StatusCode::AppendSuccess as i32);
        assert_eq!(response.message, "Block Commit appended successfully");
        //FIXME last_committed_block_number should actually be called lastlogindex assert_eq!(response.last_committed_block_number, 1);
    }

    #[tokio::test]
    async fn test_append_block_commit_leader() {
        let consensus = create_leader_consensus().await;
        let service = AppendEntryServiceImpl {
            consensus: Mutex::new(Arc::clone(&consensus)),
        };

        // Simulate the node as a leader
        consensus.set_role(Role::Leader);

        let request = Request::new(AppendBlockCommitRequest {
            term: 1,
            leader_id: "leader_id".to_string(),
            prev_log_index: 0,
            prev_log_term: 0,
            block_entry: Some(BlockEntry {
                number: 1,
                ..Default::default()
            }),
        });

        let response = service.append_block_commit(request).await;
        assert!(response.is_err());

        let status = response.unwrap_err();
        assert_eq!(status.code(), tonic::Code::Unknown);
        assert_eq!(status.message(), "append_block_commit called on leader node");
    }

    #[tokio::test]
    async fn test_request_vote() {
        let consensus = create_leader_consensus().await;
        let service = AppendEntryServiceImpl {
            consensus: Mutex::new(Arc::clone(&consensus)),
        };

        let request = Request::new(RequestVoteRequest {
            term: 1,
            candidate_id: "https://candidate:1234;4321".to_string(),
            last_log_index: 0,
            last_log_term: 0,
        });

        let response = service.request_vote(request).await;
        assert!(response.is_ok());

        let response = response.unwrap().into_inner();
        assert_eq!(response.term, 1);
        assert!(response.vote_granted);
    }
}
