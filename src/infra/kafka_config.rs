use clap::Parser;
use display_json::DebugAsJson;
use serde::Deserialize;
use serde::Serialize;

#[derive(Default, Parser, DebugAsJson, Clone, serde::Serialize, serde::Deserialize)]
pub struct KafkaConfig {
    #[arg(long = "bootstrap-servers", env = "KAFKA_BOOTSTRAP_SERVERS", conflicts_with("leader"))]
    pub bootstrap_servers: String,
    #[arg(long = "topic", env = "KAFKA_TOPIC", conflicts_with("leader"))]
    pub topic: String,
    #[arg(long = "client-id", env = "KAFKA_CLIENT_ID", conflicts_with("leader"))]
    pub client_id: String,
    #[arg(long = "group-id", env = "KAFKA_GROUP_ID", conflicts_with("leader"))]
    pub group_id: String,
}

impl KafkaConfig {
    pub fn new(bootstrap_servers: String, topic: String, client_id: String, group_id: String) -> Self {
        Self {
            bootstrap_servers,
            topic,
            client_id,
            group_id,
        }
    }
}
