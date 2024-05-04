//TODO move this onto temporary storage, it will be called from a channel
use anyhow::Result;

pub async fn gather_clients() -> Result<()> {
    // Initialize a HashMap to store pod IPs and roles
    let _pods_list = [
        "http://stratus-api-0.stratus-api.stratus-staging.svc.cluster.local:3000",
        "http://stratus-api-1.stratus-api.stratus-staging.svc.cluster.local:3000",
        "http://stratus-api-2.stratus-api.stratus-staging.svc.cluster.local:3000",
    ];

    Ok(())
}

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
use management::ResultResponse;

pub struct ClusterManagementService;

#[tonic::async_trait]
impl ClusterManagement for ClusterManagementService {
    async fn init_cluster(&self, _request: Request<Node>) -> Result<Response<ResultResponse>, Status> {
        // Mocked response for initializing a cluster
        Ok(Response::new(ResultResponse {
            success: true,
            message: "Cluster initialized (mocked).".to_string(),
        }))
    }

    async fn add_learner(&self, _request: Request<AddLearnerRequest>) -> Result<Response<ResultResponse>, Status> {
        // Mocked response for adding a learner
        Ok(Response::new(ResultResponse {
            success: true,
            message: "Learner added (mocked).".to_string(),
        }))
    }

    async fn change_membership(&self, _request: Request<ChangeMembershipRequest>) -> Result<Response<ResultResponse>, Status> {
        // Mocked response for changing membership
        Ok(Response::new(ResultResponse {
            success: true,
            message: "Membership changed (mocked).".to_string(),
        }))
    }
}

pub async fn run_server() -> Result<()> {
    let addr = "[::1]:50051".parse()?;
    let svc = ClusterManagementServer::new(ClusterManagementService);

    Server::builder().add_service(svc).serve(addr).await?;

    Ok(())
}
