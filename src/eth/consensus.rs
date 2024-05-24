use std::env;
use std::time::Duration;

use anyhow::anyhow;
use k8s_openapi::api::core::v1::Pod;
use kube::api::Api;
use kube::api::ListParams;
use kube::Client;
use serde::Deserialize;
use serde::Serialize;
use tokio::sync::mpsc::Sender;
use tokio::sync::mpsc::{self};
use tokio::time::sleep;
use tonic::transport::Channel;
use tonic::{Request, Response, Status};
pub mod raft {
    tonic::include_proto!("raft");
}
use raft::raft_service_server::{RaftService, RaftServiceServer};
use raft::{AppendEntriesRequest, AppendEntriesResponse, Entry, raft_service_client::RaftServiceClient};

use crate::config::RunWithImporterConfig;
use crate::infra::BlockchainClient;

const RETRY_ATTEMPTS: u32 = 3;
const RETRY_DELAY: Duration = Duration::from_millis(10);

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LogEntry {
    index: u64,
    data: String,
}

pub struct Consensus {
    pub sender: Sender<String>,
    leader_name: String,
    //XXX current_index: AtomicU64,
}

impl Consensus {
    //XXX for now we pick the leader name from the environment
    // the correct is to have a leader election algorithm
    pub fn new(leader_name: Option<String>) -> Self {
        let Some(_node_name) = Self::current_node() else {
            tracing::info!("No consensus module available, running in standalone mode");
            return Self::new_stand_alone();
        };

        let Some(leader_name) = leader_name else {
            tracing::info!("No leader name provided, running in standalone mode");
            return Self::new_stand_alone();
        };

        tracing::info!("Starting consensus module with leader: {}", leader_name);

        let (sender, mut receiver) = mpsc::channel::<String>(32);

        let leader_name_clone = leader_name.clone();
        tokio::spawn(async move {
            let followers = Self::discover_followers().await.expect("Failed to discover followers");

            tracing::info!(
                "Discovered followers: {}",
                followers.iter().map(|f| f.to_string()).collect::<Vec<String>>().join(", ")
            );

            while let Some(data) = receiver.recv().await {
                if Self::is_leader(leader_name_clone.clone()) {
                    //TODO add data to consensus-log-transactions
                    //TODO at the begining of temp-storage, load the consensus-log-transactions so the index becomes clear
                    tracing::info!("Received data to append: {}", data);

                    //TODO use gRPC instead of jsonrpc
                    //FIXME for now, this has no colateral efects, but it will have in the future
                    match Self::append_entries_to_followers(vec![LogEntry { index: 0, data: data.clone() }], followers.clone()).await {
                        Ok(_) => {
                            tracing::info!("Data sent to followers: {}", data);
                        }
                        Err(e) => {
                            //TODO rediscover followers on comunication error
                            tracing::error!("Failed to send data to followers: {}", e);
                        }
                    }
                }
            }
        });

        Self { leader_name, sender }
    }

    fn new_stand_alone() -> Self {
        let (sender, mut receiver) = mpsc::channel(32);

        tokio::spawn(async move {
            while let Some(data) = receiver.recv().await {
                tracing::info!("Received data: {}", data);
            }
        });

        Self {
            leader_name: "standalone".to_string(),
            sender,
        }
    }

    //FIXME TODO automate the way we gather the leader, instead of using a env var
    pub fn is_leader(leader_name: String) -> bool {
        Self::current_node().unwrap_or("".to_string()) == leader_name
    }

    pub fn is_follower(leader_name: String) -> bool {
        !Self::is_leader(leader_name)
    }

    fn current_node() -> Option<String> {
        let pod_name = env::var("MY_POD_NAME").ok()?;
        Some(pod_name.trim().to_string())
    }

    fn current_namespace() -> Option<String> {
        let namespace = env::var("NAMESPACE").ok()?;
        Some(namespace.trim().to_string())
    }

    // XXX this is a temporary solution to get the leader node
    // later we want the leader to GENERATE blocks
    // and even later we want this sync to be replaced by a gossip protocol or raft
    pub fn get_chain_url(&self, config: RunWithImporterConfig) -> (String, Option<String>) {
        if Self::is_follower(self.leader_name.clone()) {
            if let Some(namespace) = Self::current_namespace() {
                return (format!("http://{}.stratus-api.{}.svc.cluster.local:3000", self.leader_name, namespace), None);
            }
        }
        (config.online.external_rpc, config.online.external_rpc_ws)
    }

    pub async fn discover_followers() -> Result<Vec<String>, anyhow::Error> {
        let client = Client::try_default().await?;
        let pods: Api<Pod> = Api::namespaced(client, &Self::current_namespace().unwrap_or("default".to_string()));

        let lp = ListParams::default().labels("app=stratus-api");
        let pod_list = pods.list(&lp).await?;

        let mut followers = Vec::new();
        for p in pod_list.items {
            if let Some(pod_name) = p.metadata.name {
                if pod_name != Self::current_node().unwrap() {
                    if let Some(namespace) = Self::current_namespace() {
                        followers.push(format!("http://{}.stratus-api.{}.svc.cluster.local:3000", pod_name, namespace));
                    }
                }
            }
        }

        Ok(followers)
    }

    async fn append_entries(follower: String, entries: Vec<LogEntry>) -> Result<(), anyhow::Error> {
        let mut client = RaftServiceClient::connect(follower.clone()).await?;

        for attempt in 1..=RETRY_ATTEMPTS {
            let grpc_entries: Vec<Entry> = entries.iter().map(|e| Entry {
                index: e.index,
                data: e.data.clone(),
            }).collect();

            let request = Request::new(AppendEntriesRequest { entries: grpc_entries });
            let response = client.append_entries(request).await;
            match response {
                Ok(resp) => {
                    if resp.into_inner().success {
                        tracing::debug!("Entries appended to follower {}: attempt {}: success", follower, attempt);
                        return Ok(());
                    }
                }
                Err(e) => tracing::error!("Error appending entries to follower {}: attempt {}: {:?}", follower, attempt, e),
            }
            sleep(RETRY_DELAY).await;
        }

        Err(anyhow!("Failed to append entries to {} after {} attempts", follower, RETRY_ATTEMPTS))
    }

    pub async fn append_entries_to_followers(entries: Vec<LogEntry>, followers: Vec<String>) -> Result<(), anyhow::Error> {
        for entry in entries {
            for follower in &followers {
                if let Err(e) = Self::append_entries(follower, vec![entry.clone()]).await {
                    tracing::debug!("Error appending entry to follower {}: {:?}", follower, e);
                }
            }
        }
        Ok(())
    }
}
