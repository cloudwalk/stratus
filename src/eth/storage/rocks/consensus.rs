//TODO move this onto temporary storage, it will be called from a channel
use rand::thread_rng;
use rand::Rng;
use std::collections::HashMap;
use anyhow::Result;
use k8s_openapi::api::core::v1::Pod;
use kube::api::Api;
use kube::api::ListParams;
use kube::api::ResourceExt;
use kube::Client;
use serde_json::json;
use kube::api::PatchParams;
use kube::api::Patch;

use crate::infra::BlockchainClient;

pub async fn gather_clients() -> Result<()> {
    // Infer the runtime environment and try to create a Kubernetes Client
    let client = Client::try_default().await.unwrap();

    println!("searching for pods");

    // Read pods in the configured namespace into the typed interface from k8s-openapi
    let pods: Api<Pod> = Api::default_namespaced(client);
    let pods_list = pods.list(&ListParams::default()).await.unwrap();

    // Choose a random pod to be the leader
    let mut rng = thread_rng();
    let leader_index = rng.gen_range(0..pods_list.items.len());
    let leader_pod = &pods_list.items[leader_index];

    // Initialize a HashMap to store pod IPs and roles
    let mut pod_roles: HashMap<String, String> = HashMap::new();

    for (index, pod) in pods_list.items.iter().enumerate() {
        let role = if index == leader_index { "leader" } else { "follower" };
        let pod_ip = format!("http://{}:3000", pod.status.as_ref().unwrap().pod_ip.as_ref().unwrap().clone());

        println!("found pod {} with address {}", pod.name_any(), pod_ip);

        let patch = json!({
            "metadata": {
                "labels": {
                    "role": role
                }
            }
        });

        let pp = PatchParams::apply("leader-election-handler");

        pods.patch(&pod.metadata.name.as_ref().unwrap(), &pp, &Patch::Apply(&patch)).await.unwrap();

        println!("patched pod {} with role {}", pod.name_any(), role);

        if pod.name_any() != std::env::var("HOSTNAME")? {
            let chain = BlockchainClient::new(&pod_ip).await.unwrap();
            let block_number = chain.get_current_block_number().await.unwrap();

            println!("block number: {}", block_number);
        }
    }
    Ok(())
}
