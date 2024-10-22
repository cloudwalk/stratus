use anyhow::Result;
use ethereum_types::H256;
use rdkafka::producer::FutureProducer;
use rdkafka::producer::FutureRecord;
use rdkafka::ClientConfig;

use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::Hash;
use crate::infra::kafka_config::KafkaConfig;

#[derive(Clone)]
pub struct KafkaConnector {
    producer: FutureProducer,
    topic: String,
}

impl KafkaConnector {
    pub fn new(config: &KafkaConfig) -> Result<Self> {
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", config.bootstrap_servers.as_ref().unwrap())
            .set("client.id", config.client_id.as_ref().unwrap())
            .set("group.id", config.group_id.as_ref().unwrap())
            .create()?;

        Ok(Self {
            producer,
            topic: config.topic.clone().unwrap(),
        })
    }

    pub async fn send_event(&self, block: &ExternalBlock) -> Result<()> {
        let event = self.create_event()?;
        let payload = serde_json::to_string(&event)?;

        println!("payload: {}", payload);

        match self
            .producer
            .send(
                FutureRecord::to(&self.topic).payload(&payload).key(&block.hash().to_string()),
                std::time::Duration::from_secs(0),
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => Err(anyhow::anyhow!("failed to send event to kafka: {:?}", e)),
        }
    }

    fn create_event(&self) -> Result<serde_json::Value> {
        tracing::info!("create_event: {:?}", self.topic);
        // Implemente a lógica para criar o evento a partir do bloco e da execução
        // Este é apenas um exemplo básico
        let event = serde_json::json!({
            "block_number": 0,
            "block_hash": Hash(H256::zero()),
            "transaction_hash": Hash(H256::zero()),
            "status": "execution.result",
            "gas_used": 3,
        });

        Ok(event)
    }
}
