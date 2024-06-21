// append_entry_service.rs

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
use crate::eth::consensus::PeerAddress;
use crate::eth::consensus::Role;
use crate::eth::Consensus;

pub struct AppendEntryServiceImpl {
    pub consensus: Mutex<Arc<Consensus>>,
}

#[tonic::async_trait]
impl AppendEntryService for AppendEntryServiceImpl {
    async fn append_transaction_executions(
        &self,
        request: Request<AppendTransactionExecutionsRequest>,
    ) -> Result<Response<AppendTransactionExecutionsResponse>, Status> {
        let consensus = self.consensus.lock().await;
        let request_inner = request.into_inner();

        if consensus.is_leader() {
            tracing::error!(sender = request_inner.leader_id, "append_transaction_executions called on leader node");
            return Err(Status::new(
                (StatusCode::NotLeader as i32).into(),
                "append_transaction_executions called on leader node".to_string(),
            ));
        }

        let executions = request_inner.executions;
        //TODO save the transaction executions here
        //TODO send the executions to the Storage
        tracing::info!(executions = executions.len(), "appending executions");

        consensus.reset_heartbeat_signal.notify_waiters();

        Ok(Response::new(AppendTransactionExecutionsResponse {
            status: StatusCode::AppendSuccess as i32,
            message: "transaction Executions appended successfully".into(),
            last_committed_block_number: 0,
        }))
    }

    async fn append_block_commit(&self, request: Request<AppendBlockCommitRequest>) -> Result<Response<AppendBlockCommitResponse>, Status> {
        let consensus = self.consensus.lock().await;
        let request_inner = request.into_inner();

        if consensus.is_leader() {
            tracing::error!(sender = request_inner.leader_id, "append_block_commit called on leader node");
            return Err(Status::new(
                (StatusCode::NotLeader as i32).into(),
                "append_block_commit called on leader node".to_string(),
            ));
        }

        let Some(block_entry) = request_inner.block_entry else {
            return Err(Status::invalid_argument("empty block entry"));
        };

        tracing::info!(number = block_entry.number, "appending new block");

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

        consensus.reset_heartbeat_signal.notify_waiters();
        if let Ok(leader_peer_address) = PeerAddress::from_string(request_inner.leader_id) {
            consensus.update_leader(leader_peer_address).await;
        }
        consensus.last_arrived_block_number.store(block_entry.number, Ordering::SeqCst);

        tracing::info!(
            last_last_arrived_block_number = last_last_arrived_block_number,
            new_last_arrived_block_number = consensus.last_arrived_block_number.load(Ordering::SeqCst),
            "last arrived block number set",
        );

        Ok(Response::new(AppendBlockCommitResponse {
            status: StatusCode::AppendSuccess as i32,
            message: "Block Commit appended successfully".into(),
            last_committed_block_number: consensus.last_arrived_block_number.load(Ordering::SeqCst),
        }))
    }

    async fn request_vote(&self, request: Request<RequestVoteRequest>) -> Result<Response<RequestVoteResponse>, Status> {
        let request = request.into_inner();
        let consensus = self.consensus.lock().await;
        let current_term = consensus.current_term.load(Ordering::SeqCst);

        if request.term < current_term {
            tracing::info!(
                vote_granted = false,
                current_term = current_term,
                request_term = request.term,
                "requestvote received with stale term on election"
            );
            return Ok(Response::new(RequestVoteResponse {
                term: current_term,
                vote_granted: false,
            }));
        }

        let candidate_address = PeerAddress::from_string(request.candidate_id.clone()).unwrap(); //XXX FIXME replace with rpc error

        if request.term > current_term && request.last_log_index >= consensus.last_arrived_block_number.load(Ordering::SeqCst) {
            consensus.current_term.store(request.term, Ordering::SeqCst);
            consensus.set_role(Role::Follower);
            consensus.reset_heartbeat_signal.notify_waiters(); // reset the heartbeat signal to avoid election timeout just after voting

            let mut voted_for = consensus.voted_for.lock().await;
            *voted_for = Some(candidate_address.clone());

            tracing::info!(vote_granted = true, current_term = current_term, request_term = request.term, candidate_address = %candidate_address, "voted for candidate on election");
            return Ok(Response::new(RequestVoteResponse {
                term: request.term,
                vote_granted: true,
            }));
        }

        tracing::info!(vote_granted = false, current_term = current_term, request_term = request.term, candidate_address = %candidate_address, "already voted for another candidate on election");
        Ok(Response::new(RequestVoteResponse {
            term: request.term,
            vote_granted: false,
        }))
    }
}
