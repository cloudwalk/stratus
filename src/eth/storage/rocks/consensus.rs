//TODO move this onto temporary storage, it will be called from a channel

use anyhow::Result;

use crate::infra::BlockchainClient;

pub async fn gather_clients() -> Result<()> {
    // Initialize a HashMap to store pod IPs and roles
    let pods_list = [
        "http://stratus-api-0.stratus-api.stratus-staging.svc.cluster.local:3000",
        "http://stratus-api-1.stratus-api.stratus-staging.svc.cluster.local:3000",
        "http://stratus-api-2.stratus-api.stratus-staging.svc.cluster.local:3000",
    ];

    for pod_ip in pods_list.iter() {
        let chain = match BlockchainClient::new(pod_ip).await {
            Ok(chain) => chain,
            Err(e) => {
                println!("Error: {}", e);
                continue;
            }
        };
        let block_number = match chain.get_current_block_number().await {
            Ok(block_number) => block_number,
            Err(e) => {
                println!("Error: {}", e);
                continue;
            }
        };

        println!("block number: {}", block_number);
    }
    Ok(())
}
