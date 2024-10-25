use anyhow::Result;
use ethereum_types::H256;
use rdkafka::producer::FutureProducer;
use rdkafka::producer::FutureRecord;
use rdkafka::ClientConfig;

use crate::eth::primitives::Hash;

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct KafkaConfig {
    pub bootstrap_servers: Option<String>,
    pub topic: Option<String>,
    pub client_id: Option<String>,
    pub group_id: Option<String>,
}

impl KafkaConfig {
    pub fn is_configured(&self) -> bool {
        self.bootstrap_servers.is_some() && self.topic.is_some() && self.client_id.is_some() && self.group_id.is_some()
    }
}

#[derive(Clone)]
pub struct KafkaConnector {
    producer: FutureProducer,
    topic: String,
}

impl KafkaConnector {
    pub fn new(config: &KafkaConfig) -> Result<Self> {
        let producer;

        if let Some(group_id) = config.group_id.as_ref() {
            producer = ClientConfig::new()
                .set("bootstrap.servers", config.bootstrap_servers.as_ref().unwrap())
                .set("client.id", config.client_id.as_ref().unwrap())
                .set("group.id", group_id)
                .create()?;
        } else {
            producer = ClientConfig::new()
                .set("bootstrap.servers", config.bootstrap_servers.as_ref().unwrap())
                .set("client.id", config.client_id.as_ref().unwrap())
                .create()?;
        };

        Ok(Self {
            producer,
            topic: config.topic.clone().unwrap(),
        })
    }

    // fazer o send_event receber um json
    // ou seja matar o create event
    // receber o header do kafka
    pub async fn send_event(&self, event: serde_json::Value) -> Result<()> {
        let payload = serde_json::to_string(&event)?;

        match self
            .producer
            .send(
                FutureRecord::to(&self.topic).payload(&payload).key(&Hash(H256::random()).to_string()),
                std::time::Duration::from_secs(0),
            )
            .await
        {
            Ok(_) => { 
                tracing::info!(payload = payload, "event sent to kafka");
                println!("event sent to kafka {:?}", payload);
                Ok(())
            },
            Err(e) => Err(anyhow::anyhow!("failed to send event to kafka: {:?}", e)),
        }
    }

}
