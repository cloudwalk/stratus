use anyhow::Result;
use anyhow::anyhow;
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
use stratus_macros::CliOverrides;
use stratus_macros::timed;

use crate::ledger::events::Event;
use crate::log_and_err;

#[derive(Parser, DebugAsJson, Clone, serde::Serialize, serde::Deserialize, Default, CliOverrides)]
#[serde(default, deny_unknown_fields)]
pub struct KafkaConfig {
    #[arg(long = "kafka-bootstrap-servers", required = false)]
    pub bootstrap_servers: String,

    #[arg(long = "kafka-topic", group = "kafka", required = false)]
    pub topic: String,

    #[arg(long = "kafka-client-id", required = false)]
    pub client_id: String,

    #[arg(long = "kafka-group-id", required = false)]
    pub group_id: Option<String>,

    #[arg(long = "kafka-security-protocol", required = false, default_value_t)]
    pub security_protocol: KafkaSecurityProtocol,

    #[arg(long = "kafka-sasl-mechanisms", required = false)]
    pub sasl_mechanisms: Option<String>,

    #[arg(long = "kafka-sasl-username", required = false)]
    pub sasl_username: Option<String>,

    #[arg(long = "kafka-sasl-password", required = false)]
    pub sasl_password: Option<String>,

    #[arg(long = "kafka-ssl-ca-location", required = false)]
    pub ssl_ca_location: Option<String>,

    #[arg(long = "kafka-ssl-certificate-location", required = false)]
    pub ssl_certificate_location: Option<String>,

    #[arg(long = "kafka-ssl-key-location", required = false)]
    pub ssl_key_location: Option<String>,
}

impl KafkaConfig {
    pub fn init(&self) -> Result<KafkaConnector> {
        KafkaConnector::new(self)
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
        tracing::info!(
            topic = %config.topic,
            bootstrap_servers = %config.bootstrap_servers,
            client_id = %config.client_id,
            "Creating Kafka connector"
        );

        let security_protocol = config.security_protocol;
        let mut client_config = ClientConfig::new()
            .set("bootstrap.servers", &config.bootstrap_servers)
            .set("client.id", &config.client_id)
            .set("linger.ms", "5")
            .set("batch.size", "1048576") // 1 MB
            .to_owned();

        let producer = match security_protocol {
            KafkaSecurityProtocol::None => client_config.create()?,
            KafkaSecurityProtocol::SaslSsl => client_config
                .set("security.protocol", "SASL_SSL")
                .set(
                    "sasl.mechanisms",
                    config.sasl_mechanisms.as_ref().ok_or(anyhow!("sasl mechanisms is required"))?.as_str(),
                )
                .set(
                    "sasl.username",
                    config.sasl_username.as_ref().ok_or(anyhow!("sasl username is required"))?.as_str(),
                )
                .set(
                    "sasl.password",
                    config.sasl_password.as_ref().ok_or(anyhow!("sasl password is required"))?.as_str(),
                )
                .create()?,
            KafkaSecurityProtocol::Ssl => client_config
                .set(
                    "ssl.ca.location",
                    config.ssl_ca_location.as_ref().ok_or(anyhow!("ssl ca location is required"))?.as_str(),
                )
                .set(
                    "ssl.certificate.location",
                    config
                        .ssl_certificate_location
                        .as_ref()
                        .ok_or(anyhow!("ssl certificate location is required"))?
                        .as_str(),
                )
                .set(
                    "ssl.key.location",
                    config.ssl_key_location.as_ref().ok_or(anyhow!("ssl key location is required"))?.as_str(),
                )
                .create()?,
        };

        Ok(Self {
            producer,
            topic: config.topic.clone(),
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
