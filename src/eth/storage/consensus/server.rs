//TODO move this onto temporary storage, it will be called from a channel
use anyhow::Result;
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




pub mod raftpb {
    tonic::include_proto!("raftpb"); // Make sure this path is correct as per your setup
}

use raftpb::{VoteRequest, AppendEntriesRequest, InstallSnapshotRequest, ResultResponse as RaftResultResponse};
use raftpb::raft_pb_server::RaftPb;
use raftpb::raft_pb_server::RaftPbServer;

pub struct RaftPbService;

#[tonic::async_trait]
impl RaftPb for RaftPbService {
    async fn vote(&self, request: Request<VoteRequest>) -> Result<Response<RaftResultResponse>, Status> {
        // Here you would have your logic to process a vote
        Ok(Response::new(RaftResultResponse {
            success: true,
            message: "Vote processed successfully.".to_string(),
        }))
    }

    async fn append_entries(&self, request: Request<AppendEntriesRequest>) -> Result<Response<RaftResultResponse>, Status> {
        // Here you would handle appending entries to the log
        Ok(Response::new(RaftResultResponse {
            success: true,
            message: "Entries appended successfully.".to_string(),
        }))
    }

    async fn install_snapshot(&self, request: Request<InstallSnapshotRequest>) -> Result<Response<RaftResultResponse>, Status> {
        // Here you handle the installation of a snapshot
        Ok(Response::new(RaftResultResponse {
            success: true,
            message: "Snapshot installed successfully.".to_string(),
        }))
    }
}

pub async fn run_server() -> Result<()> {
    let addr = "[::1]:50051".parse()?;
    let cluster_management_svc = ClusterManagementServer::new(ClusterManagementService);
    let raft_svc = RaftPbServer::new(RaftPbService);

    Server::builder().add_service(cluster_management_svc).add_service(raft_svc).serve(addr).await?;

    Ok(())
}
