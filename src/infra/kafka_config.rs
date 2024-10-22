use clap::Parser;
use display_json::DebugAsJson;

#[derive(Default, Parser, DebugAsJson, Clone, serde::Serialize, serde::Deserialize)]
#[clap(group = clap::ArgGroup::new("kafka").multiple(true).requires_all(&["kafka-bootstrap-servers", "kafka-topic", "kafka-client-id", "kafka-group-id"]))]
pub struct KafkaConfig {
    #[arg(long = "kafka-bootstrap-servers", env = "KAFKA_BOOTSTRAP_SERVERS", group = "kafka")]
    pub bootstrap_servers: Option<String>,

    #[arg(long = "kafka-topic", env = "KAFKA_TOPIC", group = "kafka")]
    pub topic: Option<String>,

    #[arg(long = "kafka-client-id", env = "KAFKA_CLIENT_ID", group = "kafka")]
    pub client_id: Option<String>,

    #[arg(long = "kafka-group-id", env = "KAFKA_GROUP_ID", group = "kafka")]
    pub group_id: Option<String>,
}

impl KafkaConfig {
    pub fn is_configured(&self) -> bool {
        self.bootstrap_servers.is_some() && self.topic.is_some() && self.client_id.is_some() && self.group_id.is_some()
    }
}
