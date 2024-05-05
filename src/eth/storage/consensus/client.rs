use management::cluster_management_client::ClusterManagementClient;
use management::AddLearnerRequest;
use management::ChangeMembershipRequest;
use management::Node;
use tonic::transport::Channel;

pub mod management {
    tonic::include_proto!("management");
}

pub struct ManagementClient {
    client: ClusterManagementClient<Channel>,
}

impl ManagementClient {
    pub async fn connect(addr: String) -> Result<Self, tonic::transport::Error> {
        let client = ClusterManagementClient::connect(addr).await?;
        Ok(ManagementClient { client })
    }

    pub async fn init_cluster(&mut self, id: u64, address: String) -> Result<String, tonic::Status> {
        let request = Node { id, address };
        let response = self.client.init_cluster(tonic::Request::new(request)).await?;
        Ok(response.into_inner().message)
    }

    pub async fn add_learner(&mut self, id: u64, address: String) -> Result<String, tonic::Status> {
        let request = AddLearnerRequest {
            node: Some(Node { id, address }),
        };
        let response = self.client.add_learner(tonic::Request::new(request)).await?;
        Ok(response.into_inner().message)
    }

    pub async fn change_membership(&mut self, nodes: Vec<u64>) -> Result<String, tonic::Status> {
        let request = ChangeMembershipRequest { nodes };
        let response = self.client.change_membership(tonic::Request::new(request)).await?;
        Ok(response.into_inner().message)
    }
}
