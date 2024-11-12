use anyhow::Result;
use clap::Parser;
use clap::ValueEnum;
use display_json::DebugAsJson;
use futures::Stream;
use futures::StreamExt;
use rdkafka::message::Header;
use rdkafka::message::OwnedHeaders;
use rdkafka::producer::future_producer::OwnedDeliveryResult;
use rdkafka::producer::DeliveryFuture;
use rdkafka::producer::FutureProducer;
use rdkafka::producer::FutureRecord;
use rdkafka::ClientConfig;

use crate::infra::metrics;
use crate::ledger::events::Event;
use crate::log_and_err;

#[derive(Parser, DebugAsJson, Clone, serde::Serialize, serde::Deserialize, Default)]
#[group(requires_all = ["bootstrap_servers", "topic", "client_id", "ImporterConfig"])]
pub struct KafkaConfig {
    #[arg(long = "kafka-bootstrap-servers", env = "KAFKA_BOOTSTRAP_SERVERS", required = false)]
    pub bootstrap_servers: String,

    #[arg(long = "kafka-topic", env = "KAFKA_TOPIC", group = "kafka", required = false)]
    pub topic: String,

    #[arg(long = "kafka-client-id", env = "KAFKA_CLIENT_ID", required = false)]
    pub client_id: String,

    #[arg(long = "kafka-group-id", env = "KAFKA_GROUP_ID", required = false)]
    pub group_id: Option<String>,

    #[arg(long = "kafka-security-protocol", env = "KAFKA_SECURITY_PROTOCOL", required = false, default_value_t)]
    pub security_protocol: KafkaSecurityProtocol,

    #[arg(long = "kafka-sasl-mechanisms", env = "KAFKA_SASL_MECHANISMS", required = false)]
    pub sasl_mechanisms: Option<String>,

    #[arg(long = "kafka-sasl-username", env = "KAFKA_SASL_USERNAME", required = false)]
    pub sasl_username: Option<String>,

    #[arg(long = "kafka-sasl-password", env = "KAFKA_SASL_PASSWORD", required = false)]
    pub sasl_password: Option<String>,

    #[arg(long = "kafka-ssl-ca-location", env = "KAFKA_SSL_CA_LOCATION", required = false)]
    pub ssl_ca_location: Option<String>,

    #[arg(long = "kafka-ssl-certificate-location", env = "KAFKA_SSL_CERTIFICATE_LOCATION", required = false)]
    pub ssl_certificate_location: Option<String>,

    #[arg(long = "kafka-ssl-key-location", env = "KAFKA_SSL_KEY_LOCATION", required = false)]
    pub ssl_key_location: Option<String>,
}

impl KafkaConfig {
    pub fn has_credentials(&self) -> bool {
        match self.security_protocol {
            KafkaSecurityProtocol::None => true,
            KafkaSecurityProtocol::SaslSsl => self.sasl_mechanisms.is_some() && self.sasl_username.is_some() && self.sasl_password.is_some(),
            KafkaSecurityProtocol::Ssl => self.ssl_ca_location.is_some() && self.ssl_certificate_location.is_some() && self.ssl_key_location.is_some(),
        }
    }

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
    None,
    SaslSsl,
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

        let producer = match security_protocol {
            KafkaSecurityProtocol::None => ClientConfig::new()
                .set("bootstrap.servers", &config.bootstrap_servers)
                .set("client.id", &config.client_id)
                .create()?,
            KafkaSecurityProtocol::SaslSsl => ClientConfig::new()
                .set("security.protocol", "SASL_SSL")
                .set("bootstrap.servers", &config.bootstrap_servers)
                .set("client.id", &config.client_id)
                .set(
                    "sasl.mechanisms",
                    config.sasl_mechanisms.as_ref().expect("sasl mechanisms is required").as_str(),
                )
                .set("sasl.username", config.sasl_username.as_ref().expect("sasl username is required").as_str())
                .set("sasl.password", config.sasl_password.as_ref().expect("sasl password is required").as_str())
                .create()?,
            KafkaSecurityProtocol::Ssl => ClientConfig::new()
                .set("bootstrap.servers", &config.bootstrap_servers)
                .set("client.id", &config.client_id)
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

    pub fn create_buffer<T, I>(&self, events: I, buffer_size: usize) -> Result<impl Stream<Item = Result<()>>>
    where
        T: Event,
        I: IntoIterator<Item = T>,
    {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let futures: Vec<DeliveryFuture> = events
            .into_iter()
            .map(|event| {
                metrics::timed(|| self.queue_event(event)).with(|m| {
                    metrics::inc_kafka_queue_event(m.elapsed);
                })
            })
            .collect::<Result<Vec<_>, _>>()?; // This could fail because the queue is full (?)

        #[cfg(feature = "metrics")]
        metrics::inc_kafka_create_buffer(start.elapsed());

        Ok(futures::stream::iter(futures).buffered(buffer_size).map(handle_delivery_result))
    }

    pub async fn send_buffered<T, I>(&self, events: I, buffer_size: usize) -> Result<()>
    where
        T: Event,
        I: IntoIterator<Item = T>,
    {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        tracing::info!(?buffer_size, "sending events");

        let mut buffer = self.create_buffer(events, buffer_size)?;
        while let Some(res) = buffer.next().await {
            if let Err(e) = res {
                return log_and_err!(reason = e, "failed to send events");
            }
        }

        #[cfg(feature = "metrics")]
        metrics::inc_kafka_send_buffered(start.elapsed());
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
