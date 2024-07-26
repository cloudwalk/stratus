use std::cmp::Ordering::Equal;
use std::cmp::Ordering::Greater;
use std::cmp::Ordering::Less;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use tokio::sync::Mutex;
use tonic::transport::Server;
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
use super::Raft;
use crate::eth::consensus::raft::append_entry::append_entry_service_server::AppendEntryServiceServer;
use crate::eth::consensus::raft::AppendEntryService;
use crate::eth::consensus::raft::LogEntryData;
use crate::eth::consensus::raft::PeerAddress;
use crate::eth::consensus::raft::Role;
use crate::eth::miner::block_from_propagation;
use crate::eth::primitives::LocalTransactionExecution;
use crate::eth::primitives::TransactionExecution;
use crate::ext::spawn_named;
#[cfg(feature = "metrics")]
use crate::infra::metrics;
use crate::GlobalState;

#[cfg(feature = "metrics")]
mod label {
    pub(super) const APPEND_TRANSACTION_EXECUTIONS: &str = "append_transaction_executions";
    pub(super) const APPEND_BLOCK_COMMIT: &str = "append_block_commit";
    pub(super) const REQUEST_VOTE: &str = "request_vote";
}

#[allow(dead_code)]
pub fn initialize_server(consensus: Arc<Raft>) {
    const TASK_NAME: &str = "consensus::server";
    spawn_named(TASK_NAME, async move {
        tracing::info!("Starting append entry service at address: {}", consensus.grpc_address);
        let addr = consensus.grpc_address;

        let append_entry_service = AppendEntryServiceImpl {
            consensus: Mutex::new(consensus),
        };

        let shutdown = GlobalState::wait_shutdown_warn(TASK_NAME);

        let server = Server::builder()
            .add_service(AppendEntryServiceServer::new(append_entry_service))
            .serve_with_shutdown(addr, shutdown)
            .await;

        if let Err(e) = server {
            let message = GlobalState::shutdown_from("consensus", &format!("failed to create server at {}", addr));
            tracing::error!(reason = ?e, %message);
        }
    });
}

pub struct AppendEntryServiceImpl {
    pub consensus: Mutex<Arc<Raft>>,
}

#[tonic::async_trait]
impl AppendEntryService for AppendEntryServiceImpl {
    async fn append_transaction_executions(
        &self,
        request: Request<AppendTransactionExecutionsRequest>,
    ) -> Result<Response<AppendTransactionExecutionsResponse>, Status> {
        #[cfg(feature = "metrics")]
        let start = std::time::Instant::now();

        tracing::info!(
            "append_transaction_execution request received. term: {}, prev_log_index: {}, prev_log_term: {}",
            request.get_ref().term,
            request.get_ref().prev_log_index,
            request.get_ref().prev_log_term,
        );

        let consensus = self.consensus.lock().await;
        let current_term = consensus.current_term.load(Ordering::SeqCst);
        let request_inner = request.into_inner();

        if Raft::is_leader() {
            tracing::error!(sender = request_inner.leader_id, "append_transaction_executions called on leader node");
            return Err(Status::new(
                (StatusCode::NotLeader as i32).into(),
                "append_transaction_executions called on leader node".to_string(),
            ));
        }

        if request_inner.term < current_term {
            let error_message = format!("request term {} is less than current term {}", request_inner.term, current_term);
            tracing::error!(request_term = request_inner.term, current_term = current_term, "{}", &error_message);
            return Err(Status::new((StatusCode::TermMismatch as i32).into(), error_message));
        }

        if request_inner.term > current_term {
            consensus.current_term.store(request_inner.term, Ordering::SeqCst);
            if let Ok(leader_peer_address) = PeerAddress::from_string(request_inner.leader_id) {
                tracing::info!(leader_address = %leader_peer_address, "updating leader due to received term {} greater than current term {}", request_inner.term, current_term);
                consensus.update_leader(leader_peer_address).await;
            }
        }

        let executions = request_inner.executions;
        let index = request_inner.prev_log_index + 1;
        let term = request_inner.term;
        let data = LogEntryData::TransactionExecutionEntries(executions.clone());

        let last_entry = consensus.log_entries_storage.get_last_entry();
        let (last_index, last_term) = match last_entry {
            Ok(Some(entry)) => (entry.index, entry.term),
            Ok(None) => (0, 0),
            Err(e) => {
                tracing::error!("error getting last log entry: {:?}", e);
                (0, 0)
            }
        };

        match request_inner.prev_log_index.cmp(&0) {
            Greater => match consensus.log_entries_storage.get_entry(request_inner.prev_log_index) {
                Ok(Some(log_entry)) if log_entry.term != request_inner.prev_log_term => {
                    let error_message = format!(
                        "log entry term {} does not match request term {} at index {}",
                        log_entry.term, request_inner.prev_log_term, request_inner.prev_log_index
                    );
                    tracing::error!(
                        log_entry_term = log_entry.term,
                        request_term = request_inner.prev_log_term,
                        index = request_inner.prev_log_index,
                        "{}",
                        &error_message
                    );

                    return Ok(Response::new(AppendTransactionExecutionsResponse {
                        status: StatusCode::TermMismatch as i32,
                        message: "transaction Executions append failed due to term mismatch".into(),
                        match_log_index: last_index,
                        last_log_index: last_index,
                        last_log_term: last_term,
                    }));
                }
                Ok(Some(_)) => {}
                _ => {
                    let error_message = format!("no log entry found at index {}", request_inner.prev_log_index);
                    tracing::error!(index = request_inner.prev_log_index, "{}", &error_message);
                    return Ok(Response::new(AppendTransactionExecutionsResponse {
                        status: StatusCode::LogMismatch as i32,
                        message: "transaction Executions append failed due to log mismatch".into(),
                        match_log_index: last_index,
                        last_log_index: last_index,
                        last_log_term: last_term,
                    }));
                }
            },
            Equal => match consensus.log_entries_storage.get_entry(0) {
                Ok(Some(log_entry)) if log_entry.term != request_inner.prev_log_term => {
                    return Ok(Response::new(AppendTransactionExecutionsResponse {
                        status: StatusCode::LogMismatch as i32,
                        message: "transaction Executions append failed due to log mismatch at index 0".into(),
                        match_log_index: 0,
                        last_log_index: last_index,
                        last_log_term: last_term,
                    }));
                }
                Ok(Some(_)) => {}
                Ok(None) => {
                    tracing::info!("appending initial entry to empty log");
                }
                Err(e) => {
                    tracing::error!("error checking first log entry: {:?}", e);
                    return Err(Status::internal("error checking log entries"));
                }
            },
            Less => {
                return Ok(Response::new(AppendTransactionExecutionsResponse {
                    status: StatusCode::LogMismatch as i32,
                    message: "invalid prev_log_index".into(),
                    match_log_index: last_index,
                    last_log_index: last_index,
                    last_log_term: last_term,
                }));
            }
        }

        if let Err(e) = consensus.log_entries_storage.save_log_entry(index, term, data, false) {
            tracing::error!("failed to save log entry: {:?}", e);
            return Err(Status::internal("failed to save log entry"));
        }

        consensus.prev_log_index.store(index, Ordering::SeqCst);
        consensus.reset_heartbeat_signal.notify_waiters();

        tracing::info!(executions = executions.len(), "appending executions");
        // TODO commit can be run on a background
        for execution in executions {
            match TransactionExecution::from_append_entry_transaction(execution) {
                Ok(transaction_execution) => {
                    tracing::info!(hash = %transaction_execution.hash(), "appending execution");
                    match consensus.storage.save_execution(transaction_execution) {
                        Ok(_) => {
                            tracing::info!("transaction execution commited into memory successfully");
                        }
                        Err(err) => {
                            tracing::error!("failed to commit transaction execution: {:?}", err);
                            return Err(Status::internal("failed to commit transaction execution"));
                        }
                    }
                }
                Err(err) => {
                    tracing::error!("failed to append transaction execution: {:?}", err);
                    return Err(Status::internal("failed to parse transaction execution for commit"));
                }
            }
        }

        #[cfg(feature = "metrics")]
        metrics::inc_consensus_grpc_requests_finished(start.elapsed(), label::APPEND_TRANSACTION_EXECUTIONS);

        let last_log_index = consensus.prev_log_index.load(Ordering::SeqCst);
        let last_log_term = consensus.current_term.load(Ordering::SeqCst);

        Ok(Response::new(AppendTransactionExecutionsResponse {
            status: StatusCode::AppendSuccess as i32,
            message: "transaction Executions appended successfully".into(),
            match_log_index: index,
            last_log_index,
            last_log_term,
        }))
    }

    async fn append_block_commit(&self, request: Request<AppendBlockCommitRequest>) -> Result<Response<AppendBlockCommitResponse>, Status> {
        #[cfg(feature = "metrics")]
        let start = std::time::Instant::now();

        tracing::info!(
            "append_block_commit request received. block_number: {}, term: {}, prev_log_index: {}, prev_log_term: {}",
            request.get_ref().block_entry.as_ref().map(|b| b.number).unwrap_or(0),
            request.get_ref().term,
            request.get_ref().prev_log_index,
            request.get_ref().prev_log_term,
        );

        let consensus = self.consensus.lock().await;
        let current_term = consensus.current_term.load(Ordering::SeqCst);
        let request_inner = request.into_inner();

        if Raft::is_leader() {
            tracing::error!(sender = request_inner.leader_id, "append_block_commit called on leader node");
            return Err(Status::new(
                (StatusCode::NotLeader as i32).into(),
                "append_block_commit called on leader node".to_string(),
            ));
        }

        if request_inner.term < current_term {
            let error_message = format!("request term {} is less than current term {}", request_inner.term, current_term);
            tracing::error!(request_term = request_inner.term, current_term = current_term, "{}", &error_message);
            return Err(Status::new((StatusCode::TermMismatch as i32).into(), error_message));
        }

        if request_inner.term > current_term {
            consensus.current_term.store(request_inner.term, Ordering::SeqCst);
            if let Ok(leader_peer_address) = PeerAddress::from_string(request_inner.leader_id) {
                tracing::info!(leader_address = %leader_peer_address, "updating leader due to received term {} greater than current term {}", request_inner.term, current_term);
                consensus.update_leader(leader_peer_address).await;
            }
        }

        let Some(block_entry) = request_inner.block_entry else {
            return Err(Status::invalid_argument("empty block entry"));
        };

        let index = request_inner.prev_log_index + 1;
        let term = request_inner.term;
        let data = LogEntryData::BlockEntry(block_entry.clone());

        let last_entry = consensus.log_entries_storage.get_last_entry();
        let (last_index, last_term) = match last_entry {
            Ok(Some(entry)) => (entry.index, entry.term),
            Ok(None) => (0, 0),
            Err(e) => {
                tracing::error!("error getting last log entry: {:?}", e);
                (0, 0)
            }
        };

        match request_inner.prev_log_index.cmp(&0) {
            Greater => match consensus.log_entries_storage.get_entry(request_inner.prev_log_index) {
                Ok(Some(log_entry)) if log_entry.term != request_inner.prev_log_term => {
                    let error_message = format!(
                        "log entry term {} does not match request term {} at index {}",
                        log_entry.term, request_inner.prev_log_term, request_inner.prev_log_index
                    );
                    tracing::error!(
                        log_entry_term = log_entry.term,
                        request_term = request_inner.prev_log_term,
                        index = request_inner.prev_log_index,
                        "{}",
                        &error_message
                    );

                    return Ok(Response::new(AppendBlockCommitResponse {
                        status: StatusCode::TermMismatch as i32,
                        message: "block commit append failed due to term mismatch".into(),
                        match_log_index: last_index,
                        last_log_index: last_index,
                        last_log_term: last_term,
                    }));
                }
                Ok(Some(_)) => {}
                _ => {
                    let error_message = format!("No log entry found at index {}", request_inner.prev_log_index);
                    tracing::error!(index = request_inner.prev_log_index, "{}", &error_message);
                    return Ok(Response::new(AppendBlockCommitResponse {
                        status: StatusCode::LogMismatch as i32,
                        message: "block commit append failed due to log mismatch".into(),
                        match_log_index: last_index,
                        last_log_index: last_index,
                        last_log_term: last_term,
                    }));
                }
            },
            Equal => match consensus.log_entries_storage.get_entry(0) {
                Ok(Some(log_entry)) if log_entry.term != request_inner.prev_log_term => {
                    return Ok(Response::new(AppendBlockCommitResponse {
                        status: StatusCode::LogMismatch as i32,
                        message: "block commit append failed due to log mismatch at index 0".into(),
                        match_log_index: 0,
                        last_log_index: last_index,
                        last_log_term: last_term,
                    }));
                }
                Ok(Some(_)) => {}
                Ok(None) => {
                    tracing::info!("appending initial entry to empty log");
                }
                Err(e) => {
                    tracing::error!("error checking first log entry: {:?}", e);
                    return Err(Status::internal("error checking log entries"));
                }
            },
            Less => {
                return Ok(Response::new(AppendBlockCommitResponse {
                    status: StatusCode::LogMismatch as i32,
                    message: "invalid prev_log_index".into(),
                    match_log_index: last_index,
                    last_log_index: last_index,
                    last_log_term: last_term,
                }));
            }
        }
        tracing::info!(number = block_entry.number, "appending new block");

        if let Err(e) = consensus.log_entries_storage.save_log_entry(index, term, data, false) {
            tracing::error!("failed to save log entry: {:?}", e);
            return Err(Status::internal("failed to save log entry"));
        }

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

        //TODO FIXME XXX there are a lot of operations that we must do at the temp storage and maybe some at the perm storage, such as clearing the transactions and starting a new current block
        let pending_transactions = consensus.storage.pending_transactions().map_err(|e| {
            tracing::error!("Failed to get pending transactions: {:?}", e);
            Status::internal("Failed to get pending transactions")
        })?;

        let transaction_executions: Vec<LocalTransactionExecution> = pending_transactions.into_iter().filter_map(|tx| tx.as_local()).collect();

        //TODO move this logic into BlockMiner and add safety checks as consensus is or isnt on follower mode there
        let block_result = block_from_propagation(block_entry.clone(), transaction_executions);
        consensus.storage.finish_pending_block().unwrap();
        match block_result {
            Ok(block) => match consensus.miner.commit(block.clone()) {
                Ok(_) => {
                    tracing::info!(block_number = %block.header.number, "block saved successfully");
                }
                Err(err) =>
                    if err.to_string() == "block to save is not on the correct order" {
                        tracing::error!(block_number = %block.header.number, "failed to save block: block is not in the correct order. Skipping saving block because it has already been saved");
                    } else {
                        tracing::error!(block_number = %block.header.number, "failed to save block: {:?}", err);
                        return Err(Status::internal("failed to save block"));
                    },
            },
            Err(err) => {
                tracing::error!("failed to parse block: {:?}", err);
                return Err(Status::internal("failed to parse block"));
            }
        }

        consensus.prev_log_index.store(index, Ordering::SeqCst);
        consensus.last_arrived_block_number.store(block_entry.number, Ordering::SeqCst);
        consensus.reset_heartbeat_signal.notify_waiters();

        #[cfg(feature = "metrics")]
        metrics::inc_consensus_grpc_requests_finished(start.elapsed(), label::APPEND_BLOCK_COMMIT);

        let last_log_index = consensus.prev_log_index.load(Ordering::SeqCst);
        let last_log_term = consensus.current_term.load(Ordering::SeqCst);

        Ok(Response::new(AppendBlockCommitResponse {
            status: StatusCode::AppendSuccess as i32,
            message: "Block Commit appended successfully".into(),
            match_log_index: index,
            last_log_index,
            last_log_term,
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

        let candidate_last_log_index = consensus.log_entries_storage.get_last_index().unwrap();

        if request.last_log_index >= candidate_last_log_index {
            consensus.current_term.store(request.term, Ordering::SeqCst);
            Raft::set_role(Role::Follower);
            consensus.reset_heartbeat_signal.notify_waiters(); // reset the heartbeat signal to avoid election timeout just after voting

            *consensus.voted_for.lock().await = Some(candidate_address.clone());

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
                request.last_log_index, candidate_last_log_index
            ),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eth::consensus::raft::append_entry::BlockEntry;
    use crate::eth::consensus::raft::tests::factories::*;

    #[tokio::test]
    async fn test_append_transaction_executions_insert() {
        let consensus = create_mock_consensus();
        let service = AppendEntryServiceImpl {
            consensus: Mutex::new(Arc::clone(&consensus)),
        };

        Raft::set_role(Role::Follower);

        let executions = vec![create_mock_transaction_execution_entry(None)];

        let request = Request::new(AppendTransactionExecutionsRequest {
            term: 1,
            leader_id: "leader_id".to_string(),
            prev_log_index: 0,
            prev_log_term: 0,
            executions: executions.clone(),
        });

        let response = service.append_transaction_executions(request).await;
        assert!(response.is_ok(), "{}", format!("{:?}", response));

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
        let consensus = create_leader_consensus();
        let service = AppendEntryServiceImpl {
            consensus: Mutex::new(Arc::clone(&consensus)),
        };

        // Simulate the node as not a leader
        Raft::set_role(Role::Follower);

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
        assert_eq!(response.last_log_index, 1);
    }

    #[tokio::test]
    async fn test_append_transaction_executions_leader() {
        let consensus = create_leader_consensus();
        let service = AppendEntryServiceImpl {
            consensus: Mutex::new(Arc::clone(&consensus)),
        };

        // Simulate the node as a leader
        Raft::set_role(Role::Leader);

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
        let consensus = create_follower_consensus_with_leader(None).await;
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
            block_entry: Some(create_mock_block_entry(vec![], None)),
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
        let consensus = create_leader_consensus();
        let service = AppendEntryServiceImpl {
            consensus: Mutex::new(Arc::clone(&consensus)),
        };

        // Simulate the node as a leader
        Raft::set_role(Role::Leader);

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
        let consensus = create_leader_consensus();
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

    #[tokio::test]
    async fn test_append_transaction_executions_term_mismatch_on_follower() {
        // This test verifies that if a follower receives a request with a term
        // lower than its current term, it will return a TermMismatch error.
        let consensus = create_follower_consensus_with_leader(Some(2)).await;
        let service = AppendEntryServiceImpl {
            consensus: Mutex::new(Arc::clone(&consensus)),
        };

        // Send gRPC with term 1, which is less than the current term to force an error response
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
        assert_eq!(status.code(), tonic::Code::NotFound);
        assert_eq!(status.message(), "request term 1 is less than current term 2");
    }

    #[tokio::test]
    async fn test_append_block_commit_term_mismatch_on_follower() {
        // This test verifies that if a follower receives a block commit request with a term
        // lower than its current term, it will return a TermMismatch error.
        let consensus = create_follower_consensus_with_leader(Some(2)).await;
        let service = AppendEntryServiceImpl {
            consensus: Mutex::new(Arc::clone(&consensus)),
        };

        // Send gRPC with term 1, which is less than the current term to force an error response
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
        assert_eq!(status.code(), tonic::Code::NotFound);
        assert_eq!(status.message(), "request term 1 is less than current term 2");
    }
}