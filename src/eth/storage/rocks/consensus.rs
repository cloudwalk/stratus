//TODO move this onto temporary storage, it will be called from a channel
use std::collections::HashMap;
use raft::{Config, storage::MemStorage, raw_node::RawNode};
use tokio::{sync::Mutex, time::{self, Duration}};
use tracing::info;
use anyhow::Result;

use crate::infra::BlockchainClient;
use slog::{Drain, Logger, o};

fn setup_logger() -> Logger {
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::CompactFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    Logger::root(drain, o!())
}


pub async fn gather_clients() -> Result<()> {
    // Initialize a HashMap to store pod IPs and roles
    let pods_list = [
        "http://stratus-api-0.stratus-api.stratus-staging.svc.cluster.local:3000",
        "http://stratus-api-1.stratus-api.stratus-staging.svc.cluster.local:3000",
        "http://stratus-api-2.stratus-api.stratus-staging.svc.cluster.local:3000"];

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

    let urls = pods_list.iter().map(|s| s.to_string()).collect();


    let raft_node = RaftNode::new(1, urls).await?;
    raft_node.run().await;

    Ok(())
}

struct RaftNode {
    node: Mutex<RawNode<MemStorage>>,
    peers: HashMap<u64, String>,  // Maps node index to URLs for simplicity
}

impl RaftNode {
    /// Initializes a new Raft node using URLs for peers. Assigns IDs based on order.
    pub async fn new(my_id: u64, urls: Vec<String>) -> Result<Self> {
        let mut peers = HashMap::new();
        for (id, url) in urls.iter().enumerate() {
            peers.insert(id as u64 + 1, url.clone());  // Node IDs are 1-indexed
        }

        let config = Config {
            id: my_id,
            election_tick: 10,
            heartbeat_tick: 3,
            ..Default::default()
        };
        config.validate()?;
        let storage = MemStorage::new_with_conf_state((peers.keys().cloned().collect::<Vec<u64>>(), vec![]));


        let logger = setup_logger();
        let node = RawNode::new(&config, storage, &logger)?;

        Ok(Self {
            node: Mutex::new(node),
            peers,
        })
    }

    pub async fn run(&self) {
        let timeout = Duration::from_millis(100);
        let mut interval = time::interval(timeout);
        loop {
            interval.tick().await;
            let mut node = self.node.lock().await;
            node.tick();
            if node.has_ready() {
                self.handle_ready(&mut node).await;
            }
        }
    }

    async fn handle_ready(&self, node: &mut RawNode<MemStorage>) {
        let ready = node.ready();
        if let Some(hs) = ready.hs() {
            if hs.get_term() != node.raft.term {
                info!("Term changed to {}", hs.get_term());
                let node_id = {
                    let node_guard = self.node.lock().await;  // Lock the mutex asynchronously
                    node_guard.raft.id  // Access the id of the Raft node
                };
                if hs.get_vote() == node_id {
                    info!("Node {} became leader in term {}", node_id, hs.get_term());
                } else {
                    info!("Node {} observed new leader {} in term {}", node_id, hs.get_vote(), hs.get_term());
                }
            }
        }

        for msg in ready.messages() {
            if let Some(_url) = self.peers.get(&msg.to) {
                // Serialize the message manually to JSON
                //XXX let msg_json = to_string(&msg).unwrap_or_else(|_| "{}".to_string()); // Handle error more gracefully in production
                //XXX let send_future = self.http_client.post(url)
                //XXX                                   .header("Content-Type", "application/json")
                //XXX                                   .body(msg_json)
                //XXX                                   .send();
                //XXX if let Err(e) = send_future.await {
                //XXX     error!("Failed to send Raft message: {:?}", e);
                //XXX }
            }
        }
        node.advance(ready);
    }
}
