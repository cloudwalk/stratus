//TODO move this onto temporary storage, it will be called from a channel
use anyhow::Result;
use crate::eth::storage::consensus::state_machine_store::StateMachineStore;
use openraft::Config;
use openraft::Raft;
use tonic::transport::Server;
use tonic::Request;
use tonic::Response;
use tonic::Status;

pub mod management {
    tonic::include_proto!("management"); // Make sure this path is correct as per your setup
}

use management::cluster_management_server::ClusterManagement;
use management::cluster_management_server::ClusterManagementServer;
use management::AddLearnerRequest;
use management::ChangeMembershipRequest;
use management::Node;
use management::ResultResponse as ManagementResultResponse;

pub struct ClusterManagementService;

#[tonic::async_trait]
impl ClusterManagement for ClusterManagementService {
    async fn init_cluster(&self, _request: Request<Node>) -> Result<Response<ManagementResultResponse>, Status> {
        // Mocked response for initializing a cluster
        Ok(Response::new(ManagementResultResponse {
            success: true,
            message: "Cluster initialized (mocked).".to_string(),
        }))
    }

    async fn add_learner(&self, _request: Request<AddLearnerRequest>) -> Result<Response<ManagementResultResponse>, Status> {
        Ok(Response::new(ManagementResultResponse {
            success: true,
            message: "Learner added (mocked).".to_string(),
        }))
    }

    async fn change_membership(&self, _request: Request<ChangeMembershipRequest>) -> Result<Response<ManagementResultResponse>, Status> {
        Ok(Response::new(ManagementResultResponse {
            success: true,
            message: "Membership changed (mocked).".to_string(),
        }))
    }
}

///////
///////
///////
pub mod raftpb {
    tonic::include_proto!("raftpb"); // Make sure this path is correct as per your setup
}

use std::io::Cursor;
use std::sync::Arc;

use raftpb::raft_pb_server::RaftPb;
use raftpb::raft_pb_server::RaftPbServer;
use raftpb::AppendEntriesRequest;
use raftpb::InstallSnapshotRequest;
use raftpb::ResultResponse as RaftResultResponse;
use raftpb::VoteRequest;
use serde::Deserialize;
use serde::Serialize;

openraft::declare_raft_types!(
    pub TypeConfig: D = super::state_machine_store::Request, R = super::state_machine_store::Response
);

pub struct RaftPbService {
    raft: Arc<Raft<TypeConfig>>,
}

impl RaftPbService {
    pub async fn new(node_id: u64) -> Self {
        // Configure Raft Node
        let config = Config {
            heartbeat_interval: 500,
            election_timeout_min: 1500,
            election_timeout_max: 3000,
            ..Default::default()
        };
        let config = Arc::new(config.validate().unwrap());

        // Create memory-based stores for simplicity, or define your own.
        let log_store = super::log_store::LogStore::<TypeConfig>::default();
        let state_machine_store = Arc::new(StateMachineStore::default());
        let raft = Raft::new(node_id, config, network, log_store, state_machine_store)
            .await
            .expect("Failed to create Raft node");

        Self { raft: Arc::new(raft) }
    }
}

#[tonic::async_trait]
impl RaftPb for RaftPbService {
    async fn vote(&self, _request: Request<VoteRequest>) -> Result<Response<RaftResultResponse>, Status> {
        // Here you would have your logic to process a vote
        Ok(Response::new(RaftResultResponse {
            success: true,
            message: "Vote processed successfully.".to_string(),
        }))
    }

    async fn append_entries(&self, _request: Request<AppendEntriesRequest>) -> Result<Response<RaftResultResponse>, Status> {
        // Here you would handle appending entries to the log
        Ok(Response::new(RaftResultResponse {
            success: true,
            message: "Entries appended successfully.".to_string(),
        }))
    }

    async fn install_snapshot(&self, _request: Request<InstallSnapshotRequest>) -> Result<Response<RaftResultResponse>, Status> {
        // Here you handle the installation of a snapshot
        Ok(Response::new(RaftResultResponse {
            success: true,
            message: "Snapshot installed successfully.".to_string(),
        }))
    }
}

////////
////////
////////
pub async fn run_server() -> Result<()> {
    let addr = "[::1]:50051".parse()?;
    let cluster_management_svc = ClusterManagementServer::new(ClusterManagementService);
    let raft_svc = RaftPbServer::new(RaftPbService::new(1).await);

    Server::builder().add_service(cluster_management_svc).add_service(raft_svc).serve(addr).await?;

    Ok(())
}
