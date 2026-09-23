use anyhow::Result;
use clap::Parser;
use clap::ValueEnum;
use display_json::DebugAsJson;
use futures::Stream;
use futures::StreamExt;
use rdkafka::ClientConfig;
use rdkafka::message::Header;
use rdkafka::message::OwnedHeaders;
use rdkafka::producer::DeliveryFuture;
use rdkafka::producer::FutureProducer;
use rdkafka::producer::FutureRecord;
use rdkafka::producer::future_producer::OwnedDeliveryResult;
use stratus_metrics::timed;

use crate::ext::parse_non_empty;
use crate::ledger::events::Event;
use crate::log_and_err;

#[derive(Parser, DebugAsJson, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(default)]
pub struct KafkaConfig {
    /// Kafka bootstrap servers.
    #[arg(id = "kafka.bootstrap_servers", long = "kafka-bootstrap-servers", value_parser = parse_non_empty, required = false)]
    pub bootstrap_servers: Option<String>,

    #[arg(id = "kafka.topic", long = "kafka-topic", group = "kafka", value_parser = parse_non_empty, required = false)]
    pub topic: Option<String>,

    #[arg(id = "kafka.client_id", long = "kafka-client-id", value_parser = parse_non_empty, required = false)]
    pub client_id: Option<String>,

    #[arg(id = "kafka.group_id", long = "kafka-group-id", required = false)]
    pub group_id: Option<String>,

    #[arg(id = "kafka.security_protocol", long = "kafka-security-protocol", required = false, default_value_t)]
    pub security_protocol: KafkaSecurityProtocol,

    #[arg(id = "kafka.sasl_mechanisms", long = "kafka-sasl-mechanisms", required = false)]
    pub sasl_mechanisms: Option<String>,

    #[arg(id = "kafka.sasl_username", long = "kafka-sasl-username", required = false)]
    pub sasl_username: Option<String>,

    #[arg(id = "kafka.sasl_password", long = "kafka-sasl-password", required = false)]
    pub sasl_password: Option<String>,

    #[arg(id = "kafka.ssl_ca_location", long = "kafka-ssl-ca-location", required = false)]
    pub ssl_ca_location: Option<String>,

    #[arg(id = "kafka.ssl_certificate_location", long = "kafka-ssl-certificate-location", required = false)]
    pub ssl_certificate_location: Option<String>,

    #[arg(id = "kafka.ssl_key_location", long = "kafka-ssl-key-location", required = false)]
    pub ssl_key_location: Option<String>,
}

/// The `[kafka]` section is missing fields required by its configured security protocol.
#[derive(Debug, thiserror::Error)]
#[error("incomplete `[kafka]` configuration: add {fields}")]
pub struct IncompleteKafkaConfig {
    fields: String,
}

impl KafkaConfig {
    pub fn init(&self) -> Result<KafkaConnector> {
        KafkaConnector::new(self)
    }

    /// Validates the fields required by the configured security protocol.
    pub fn validate(&self) -> Result<(), IncompleteKafkaConfig> {
        let mut missing: Vec<&str> = Vec::new();
        for (field, value) in [
            ("kafka.bootstrap_servers", &self.bootstrap_servers),
            ("kafka.topic", &self.topic),
            ("kafka.client_id", &self.client_id),
        ] {
            if value.is_none() {
                missing.push(field);
            }
        }
        match self.security_protocol {
            KafkaSecurityProtocol::SaslSsl => {
                for (field, value) in [
                    ("kafka.sasl_mechanisms", &self.sasl_mechanisms),
                    ("kafka.sasl_username", &self.sasl_username),
                    ("kafka.sasl_password", &self.sasl_password),
                ] {
                    if value.is_none() {
                        missing.push(field);
                    }
                }
            }
            KafkaSecurityProtocol::Ssl => {
                for (field, value) in [
                    ("kafka.ssl_ca_location", &self.ssl_ca_location),
                    ("kafka.ssl_certificate_location", &self.ssl_certificate_location),
                    ("kafka.ssl_key_location", &self.ssl_key_location),
                ] {
                    if value.is_none() {
                        missing.push(field);
                    }
                }
            }
            KafkaSecurityProtocol::None => {}
        }

        if missing.is_empty() {
            return Ok(());
        }
        let fields = missing.iter().map(|field| format!("`{field}`")).collect::<Vec<_>>().join(", ");
        Err(IncompleteKafkaConfig { fields })
    }
}

#[derive(Clone)]
pub struct KafkaConnector {
    producer: FutureProducer,
    topic: String,
}

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize, ValueEnum, Default)]
pub enum KafkaSecurityProtocol {
    #[default]
    #[serde(rename = "none")]
    None,

    #[serde(rename = "sasl-ssl")]
    SaslSsl,

    #[serde(rename = "ssl")]
    Ssl,
}

impl std::fmt::Display for KafkaSecurityProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KafkaSecurityProtocol::None => write!(f, "none"),
            KafkaSecurityProtocol::SaslSsl => write!(f, "sasl_ssl"),
            KafkaSecurityProtocol::Ssl => write!(f, "ssl"),
        }
    }
}

impl KafkaConnector {
    pub fn new(config: &KafkaConfig) -> Result<Self> {
        config.validate()?;

        let bootstrap_servers = config.bootstrap_servers.as_deref().unwrap();
        let topic = config.topic.as_deref().unwrap();
        let client_id = config.client_id.as_deref().unwrap();

        tracing::info!(
            topic = %topic,
            bootstrap_servers = %bootstrap_servers,
            client_id = %client_id,
            "Creating Kafka connector"
        );

        let security_protocol = config.security_protocol;
        let mut client_config = ClientConfig::new()
            .set("bootstrap.servers", bootstrap_servers)
            .set("client.id", client_id)
            .set("linger.ms", "5")
            .set("batch.size", "1048576") // 1 MB
            .to_owned();

        let producer = match security_protocol {
            KafkaSecurityProtocol::None => client_config.create()?,
            KafkaSecurityProtocol::SaslSsl => client_config
                .set("security.protocol", "SASL_SSL")
                .set("sasl.mechanisms", config.sasl_mechanisms.as_deref().unwrap())
                .set("sasl.username", config.sasl_username.as_deref().unwrap())
                .set("sasl.password", config.sasl_password.as_deref().unwrap())
                .create()?,
            KafkaSecurityProtocol::Ssl => client_config
                .set("ssl.ca.location", config.ssl_ca_location.as_deref().unwrap())
                .set("ssl.certificate.location", config.ssl_certificate_location.as_deref().unwrap())
                .set("ssl.key.location", config.ssl_key_location.as_deref().unwrap())
                .create()?,
        };

        Ok(Self {
            producer,
            topic: topic.to_string(),
        })
    }

    pub fn queue_event<T: Event>(&self, event: T) -> Result<DeliveryFuture> {
        tracing::debug!(?event, "queueing event");

        // prepare base payload
        let headers = event.event_headers()?;
        let key = event.event_key()?;
        let payload = event.event_payload()?;

        // prepare kafka payload
        let mut kafka_headers = OwnedHeaders::new_with_capacity(headers.len());
        for (key, value) in headers.iter() {
            let header = Header { key, value: Some(value) };
            kafka_headers = kafka_headers.insert(header);
        }
        let kafka_record = FutureRecord::to(&self.topic).payload(&payload).key(&key).headers(kafka_headers);

        // publis and handle response
        tracing::info!(%key, %payload, ?headers, "publishing kafka event");
        match self.producer.send_result(kafka_record) {
            Err((e, _)) => log_and_err!(reason = e, "failed to queue kafka event"),
            Ok(fut) => Ok(fut),
        }
    }

    pub async fn send_event<T: Event>(&self, event: T) -> Result<()> {
        tracing::debug!(?event, "sending event");
        handle_delivery_result(self.queue_event(event)?.await)
    }

    #[timed(kafka_create_buffer)]
    pub fn create_buffer<T, I>(&self, events: I, buffer_size: usize) -> Result<impl Stream<Item = Result<()>>>
    where
        T: Event,
        I: IntoIterator<Item = T>,
    {
        let futures: Vec<DeliveryFuture> = events.into_iter().map(|event| self.queue_event(event)).collect::<Result<Vec<_>, _>>()?; // This could fail because the queue is full (?)

        Ok(futures::stream::iter(futures).buffered(buffer_size).map(handle_delivery_result))
    }

    #[timed(kafka_send_buffered)]
    pub async fn send_buffered<T, I>(&self, events: I, buffer_size: usize) -> Result<()>
    where
        T: Event,
        I: IntoIterator<Item = T>,
    {
        tracing::info!(?buffer_size, "sending events");

        let mut buffer = self.create_buffer(events, buffer_size)?;
        while let Some(res) = buffer.next().await {
            if let Err(e) = res {
                return log_and_err!(reason = e, "failed to send events");
            }
        }

        Ok(())
    }
}

fn handle_delivery_result(res: Result<OwnedDeliveryResult, futures_channel::oneshot::Canceled>) -> Result<()> {
    match res {
        Err(e) => log_and_err!(reason = e, "failed to publish kafka event"),
        Ok(Err((e, _))) => log_and_err!(reason = e, "failed to publish kafka event"),
        Ok(_) => Ok(()),
    }
}
