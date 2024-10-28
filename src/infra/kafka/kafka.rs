use anyhow::Result;
use clap::ValueEnum;
use ethereum_types::H256;
use rdkafka::producer::FutureProducer;
use rdkafka::producer::FutureRecord;
use rdkafka::ClientConfig;

use crate::eth::primitives::Hash;
use crate::ledger::events::Event;

#[derive(Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct KafkaConfig {
    pub bootstrap_servers: Option<String>,
    pub topic: Option<String>,
    pub client_id: Option<String>,
    pub group_id: Option<String>,
    pub security_protocol: Option<KafkaSecurityProtocol>,
    pub sasl_mechanisms: Option<String>,
    pub sasl_username: Option<String>,
    pub sasl_password: Option<String>,
    pub ssl_ca_location: Option<String>,
    pub ssl_certificate_location: Option<String>,
    pub ssl_key_location: Option<String>,
}

impl KafkaConfig {
    pub fn has_kafka_config(&self) -> bool {
        match self.security_protocol {
            Some(KafkaSecurityProtocol::None) => self.bootstrap_servers.is_some() && self.topic.is_some() && self.client_id.is_some(),
            Some(KafkaSecurityProtocol::SaslSsl) =>
                self.bootstrap_servers.is_some()
                    && self.topic.is_some()
                    && self.client_id.is_some()
                    && self.sasl_mechanisms.is_some()
                    && self.sasl_username.is_some()
                    && self.sasl_password.is_some(),
            Some(KafkaSecurityProtocol::Ssl) =>
                self.bootstrap_servers.is_some()
                    && self.topic.is_some()
                    && self.client_id.is_some()
                    && self.ssl_ca_location.is_some()
                    && self.ssl_certificate_location.is_some()
                    && self.ssl_key_location.is_some(),
            None => false,
        }
    }
}

#[derive(Clone)]
pub struct KafkaConnector {
    producer: FutureProducer,
    topic: String,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, ValueEnum)]
pub enum KafkaSecurityProtocol {
    None,
    SaslSsl,
    Ssl,
}

impl KafkaConnector {
    pub fn new(config: &KafkaConfig) -> Result<Self> {
        let security_protocol = config.security_protocol.clone().unwrap_or(KafkaSecurityProtocol::None);

        let producer = match security_protocol {
            KafkaSecurityProtocol::None => ClientConfig::new()
                .set("bootstrap.servers", config.bootstrap_servers.as_ref().unwrap())
                .set("client.id", config.client_id.as_ref().unwrap())
                .create()?,
            KafkaSecurityProtocol::SaslSsl => ClientConfig::new()
                .set("security.protocol", "SASL_SSL")
                .set("bootstrap.servers", config.bootstrap_servers.as_ref().unwrap())
                .set("client.id", config.client_id.as_ref().unwrap())
                .set(
                    "sasl.mechanisms",
                    config.sasl_mechanisms.as_ref().expect("sasl mechanisms is required").as_str(),
                )
                .set("sasl.username", config.sasl_username.as_ref().expect("sasl username is required").as_str())
                .set("sasl.password", config.sasl_password.as_ref().expect("sasl password is required").as_str())
                .create()?,
            KafkaSecurityProtocol::Ssl => ClientConfig::new()
                .set("bootstrap.servers", config.bootstrap_servers.as_ref().unwrap())
                .set("client.id", config.client_id.as_ref().unwrap())
                .set(
                    "ssl.ca.location",
                    config.ssl_ca_location.as_ref().expect("ssl ca location is required").as_str(),
                )
                .set(
                    "ssl.certificate.location",
                    config.ssl_certificate_location.as_ref().expect("ssl certificate location is required").as_str(),
                )
                .set(
                    "ssl.key.location",
                    config.ssl_key_location.as_ref().expect("ssl key location is required").as_str(),
                )
                .create()?,
        };

        Ok(Self {
            producer,
            topic: config.topic.clone().unwrap(),
        })
    }

    pub async fn send_event<T: Event>(&self, event: T) -> Result<()> {
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
            }
            Err(e) => Err(anyhow::anyhow!("failed to send event to kafka: {:?}", e)),
        }
    }
}
