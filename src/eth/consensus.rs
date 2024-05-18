use kube::{api::Api, Client};
use kube::api::ListParams;
use k8s_openapi::api::core::v1::Pod;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;
use std::env;

use crate::config::RunWithImporterConfig;

#[derive(Debug, Serialize, Deserialize)]
struct Entry {
    index: u64,
    data: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct AppendEntriesRequest {
    entries: Vec<Entry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AppendEntriesResponse {
    success: bool,
}

pub struct Consensus {
    node_name: String,
    leader_name: String,
    //XXX retry_attempts: u32,
    //XXX retry_delay: Duration,
}

impl Consensus {
    //XXX for now we pick the leader name from the environment
    // the correct is to have a leader election algorithm
    pub fn new(leader_name: Option<String>) -> Self {
        let node_name = match Self::current_node() {
            Some(node_name) => node_name,
            None => {
                tracing::info!("No consensus module available, running in standalone mode");
                return Self::new_stand_alone();
            }
        };

        let leader_name = match leader_name {
            Some(leader_name) => leader_name,
            None => {
                tracing::info!("No leader name provided, running in standalone mode");
                return Self::new_stand_alone();
            }
        };

        Self {
            node_name,
            leader_name,
            //XXX  retry_attempts: 3,
            //XXX  retry_delay: Duration::from_millis(10),
        }
    }

    fn new_stand_alone() -> Self {
        Self {
            node_name: "standalone".to_string(),
            leader_name: "standalone".to_string(),
            //XXX retry_attempts: 0,
            //XXX retry_delay: Duration::from_millis(0),
        }
    }

    pub fn is_leader(&self) -> bool {
        self.node_name == self.leader_name
    }

    pub fn is_follower(&self) -> bool {
        !self.is_leader()
    }

    fn current_node() -> Option<String> {
        let mut file = File::open("/etc/hostname").ok()?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).ok()?;
        Some(contents.trim().to_string())
    }

    fn current_namespace() -> Option<String> {
        let namespace = env::var("NAMESPACE").ok()?;
        Some(namespace.trim().to_string())
    }

    // XXX this is a temporary solution to get the leader node
    // later we want the leader to GENERATE blocks
    // and even later we want this sync to be replaced by a gossip protocol or raft
    pub fn get_chain_url(&self, config: RunWithImporterConfig) -> (String, Option<String>) {
        if self.is_follower() {
            if let Some(namespace) = Self::current_namespace() {
                return (format!("http://{}.stratus-api.{}.svc.cluster.local:3000", self.leader_name, namespace), None);
            }
        }
        (config.online.external_rpc, config.online.external_rpc_ws)
    }

    pub async fn discover_followers(&self) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let client = Client::try_default().await?;
        let pods: Api<Pod> = Api::namespaced(client, &Self::current_namespace().unwrap_or("default".to_string()));

        let lp = ListParams::default().labels("app=stratus-api");
        let pod_list = pods.list(&lp).await?;

        let mut followers = Vec::new();
        for p in pod_list.items {
            if let Some(pod_name) = p.metadata.name {
                if pod_name != self.node_name {
                    followers.push(pod_name);
                }
            }
        }

        Ok(followers)
    }

    //XXX this will be used to send the entries to the followers
    //XXX async fn append_entries(&self, follower: &str, entries: Vec<Entry>) -> Result<(), Box<dyn std::error::Error>> {
    //XXX     let client = HttpClient::new();
    //XXX     let url = format!("http://{}/append_entries", follower);

    //XXX     let request = AppendEntriesRequest {
    //XXX         entries,
    //XXX     };

    //XXX     for attempt in 1..=self.retry_attempts {
    //XXX         let response = client.post(&url).json(&request).send().await;
    //XXX         match response {
    //XXX             Ok(resp) if resp.status().is_success() => {
    //XXX                 let response: AppendEntriesResponse = resp.json().await?;
    //XXX                 if response.success {
    //XXX                     return Ok(());
    //XXX                 } else {
    //XXX                     eprintln!("AppendEntries to {} failed", follower);
    //XXX                 }
    //XXX             }
    //XXX             Ok(resp) => eprintln!("Failed to append entries to {}: {:?}", follower, resp),
    //XXX             Err(e) => eprintln!("Error appending entries to follower {}: attempt {}: {:?}", follower, attempt, e),
    //XXX         }
    //XXX         sleep(self.retry_delay).await;
    //XXX     }

    //XXX     Err(format!("Failed to append entries to {} after {} attempts", follower, self.retry_attempts).into())
    //XXX }

    //XXX pub async fn append_entries_to_followers(&self, entries: Vec<Entry>, followers: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    //XXX     for entry in entries {
    //XXX         for follower in &followers {
    //XXX             if let Err(e) = self.append_entries(follower, vec![entry.clone()]).await {
    //XXX                 eprintln!("Error appending entry to follower {}: {:?}", follower, e);
    //XXX             }
    //XXX         }
    //XXX     }
    //XXX     Ok(())
    //XXX }
}
