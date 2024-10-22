use clap::ArgGroup;
use clap::Parser;
use display_json::DebugAsJson;

#[derive(Default, Parser, DebugAsJson, Clone, serde::Serialize, serde::Deserialize)]
#[clap(group = ArgGroup::new("kafka").required(false))]
pub struct KafkaConfig {
    #[arg(long = "bootstrap-servers", env = "KAFKA_BOOTSTRAP_SERVERS", conflicts_with("leader"), group = "kafka")]
    pub bootstrap_servers: Option<String>,
    #[arg(long = "topic", env = "KAFKA_TOPIC", conflicts_with("leader"), group = "kafka")]
    pub topic: Option<String>,
    #[arg(long = "client-id", env = "KAFKA_CLIENT_ID", conflicts_with("leader"), group = "kafka")]
    pub client_id: Option<String>,
    #[arg(long = "group-id", env = "KAFKA_GROUP_ID", conflicts_with("leader"), group = "kafka")]
    pub group_id: Option<String>,
}

