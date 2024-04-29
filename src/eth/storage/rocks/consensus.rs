use anyhow::Result;
use k8s_openapi::api::core::v1::Pod;
use kube::api::Api;
use kube::api::ListParams;
use kube::api::ResourceExt;
use kube::Client;
use crate::infra::BlockchainClient;

pub async fn gather_clients() -> Result<()> {
    // Infer the runtime environment and try to create a Kubernetes Client
    let client = Client::try_default().await.unwrap();

    println!("searching for pods");

    // Read pods in the configured namespace into the typed interface from k8s-openapi
    let pods: Api<Pod> = Api::default_namespaced(client);
    let pods_list = pods.list(&ListParams::default()).await.unwrap();
    for pod in pods_list {
        let pod_ip = format!("http://{}", pod.status.as_ref().unwrap().pod_ip.as_ref().unwrap().clone());

        println!("found pod {} with address {}", pod.name_any(), pod_ip);

        let chain = BlockchainClient::new(&pod_ip).await.unwrap();
        let block_number = chain.get_current_block_number().await.unwrap();

        println!("block number: {}", block_number);
    }
    Ok(())
}
