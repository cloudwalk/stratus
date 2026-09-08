//! Application configuration.
//!
//! Configuration is loaded from a TOML file and can be overridden by explicitly provided CLI arguments.
//! See [`crate::config::loader`] for the loading rules.

pub mod loader;

use std::str::FromStr;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::anyhow;
use clap::ArgGroup;
use clap::Parser;
use display_json::DebugAsJson;
pub use loader::ConfigLoad;
use stratus_macros::CliOverrides;
use strum::VariantNames;
use tokio::runtime::Builder;
use tokio::runtime::Runtime;

use crate::eth::executor::ExecutorConfig;
use crate::eth::follower::importer::ImporterConfig;
use crate::eth::miner::MinerConfig;
use crate::eth::rpc::RpcServerConfig;
use crate::eth::storage::StorageConfig;
use crate::infra::kafka::KafkaConfig;
use crate::infra::metrics::MetricsConfig;
use crate::infra::sentry::SentryConfig;
use crate::infra::tracing::TracingConfig;

// -----------------------------------------------------------------------------
// Config: Common
// -----------------------------------------------------------------------------

pub trait WithCommonConfig {
    fn common(&self) -> &CommonConfig;
}

/// Configuration that can be used by any binary.
#[derive(DebugAsJson, Clone, Parser, serde::Deserialize, serde::Serialize, CliOverrides)]
#[serde(default, deny_unknown_fields)]
#[command(author, version, about, long_about = None)]
pub struct CommonConfig {
    /// Environment where the application is running.
    #[arg(long = "env", default_value = "local")]
    pub env: Environment,

    /// Number of threads to execute global async tasks.
    #[arg(long = "async-threads", default_value = "32")]
    #[serde(rename = "async_threads")]
    pub num_async_threads: usize,

    /// Number of threads to execute global blocking tasks.
    #[arg(long = "blocking-threads", default_value = "512")]
    #[serde(rename = "blocking_threads")]
    pub num_blocking_threads: usize,

    #[clap(flatten)]
    pub tracing: TracingConfig,

    #[clap(flatten)]
    pub sentry: Option<SentryConfig>,

    #[clap(flatten)]
    pub metrics: MetricsConfig,

    /// Prevents clap from breaking when passing `nocapture` options in tests.
    #[arg(long = "nocapture")]
    #[serde(skip)]
    pub nocapture: bool,

    /// Enables or disables unknown client interactions.
    #[arg(long = "unknown-client-enabled", default_value = "true")]
    pub unknown_client_enabled: bool,

    /// Comma-separated list of client names that are blocked from interacting with the application.
    /// Client names are matched the same way as the `app`/`client` identification headers/params.
    #[arg(long = "blocked-clients", value_delimiter = ',')]
    pub blocked_clients: Vec<String>,
}

impl Default for CommonConfig {
    fn default() -> Self {
        Self {
            env: Environment::Local,
            num_async_threads: 32,
            num_blocking_threads: 512,
            tracing: TracingConfig::default(),
            sentry: None,
            metrics: MetricsConfig::default(),
            nocapture: false,
            unknown_client_enabled: true,
            blocked_clients: Vec::new(),
        }
    }
}

impl WithCommonConfig for CommonConfig {
    fn common(&self) -> &CommonConfig {
        self
    }
}

impl CommonConfig {
    /// Initializes Tokio runtime.
    pub fn init_tokio_runtime(&self) -> anyhow::Result<Runtime> {
        println!(
            "creating tokio runtime | async_threads={} blocking_threads={}",
            self.num_async_threads, self.num_blocking_threads
        );

        let num_async_threads = self.num_async_threads;
        let num_blocking_threads = self.num_blocking_threads;
        let result = Builder::new_multi_thread()
            .enable_all()
            .worker_threads(num_async_threads)
            .max_blocking_threads(num_blocking_threads)
            .thread_keep_alive(Duration::from_secs(u64::MAX))
            .thread_name_fn(move || {
                // Tokio first create all async threads, then all blocking threads.
                // Threads are not expected to die because Tokio catches panics and blocking threads are configured to never die.
                // If one of these premises are not true anymore, this will possibly categorize threads wrongly.

                static ASYNC_ID: AtomicUsize = AtomicUsize::new(1);
                static BLOCKING_ID: AtomicUsize = AtomicUsize::new(1);

                // identify async threads
                let async_id = ASYNC_ID.fetch_add(1, Ordering::SeqCst);
                if async_id <= num_async_threads {
                    if cfg!(feature = "flamegraph") {
                        return "tokio-async".to_string();
                    } else {
                        return format!("tokio-async-{async_id}");
                    }
                }

                // identify blocking threads
                let blocking_id = BLOCKING_ID.fetch_add(1, Ordering::SeqCst);
                if cfg!(feature = "flamegraph") {
                    "tokio-blocking".to_string()
                } else {
                    format!("tokio-blocking-{blocking_id}")
                }
            })
            .build();

        match result {
            Ok(runtime) => Ok(runtime),
            Err(e) => {
                println!("failed to create tokio runtime | reason={e:?}");
                Err(e.into())
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Config: Stratus
// -----------------------------------------------------------------------------

/// Configuration for main Stratus service.
#[derive(DebugAsJson, Clone, Default, Parser, derive_more::Deref, serde::Deserialize, serde::Serialize, CliOverrides)]
#[serde(default, deny_unknown_fields)]
#[clap(group = ArgGroup::new("mode").args(&["leader", "follower", "fake_leader"]))]
pub struct StratusConfig {
    #[arg(long = "leader", conflicts_with_all = ["follower", "fake_leader", "ImporterConfig"])]
    pub leader: bool,

    #[arg(long = "follower", conflicts_with_all = ["leader", "fake_leader"])]
    pub follower: bool,

    /// The fake leader imports blocks like a follower, but executes the blocks's txs locally like a leader.
    #[arg(long = "fake-leader", conflicts_with_all = ["leader", "follower"])]
    pub fake_leader: bool,

    /// Path to the TOML configuration file. When absent, `config/{binary}.{env}.toml` is used.
    #[arg(long = "config", value_name = "FILE")]
    #[serde(skip)]
    pub config_path: Option<String>,

    #[clap(flatten)]
    #[serde(rename = "rpc")]
    pub rpc_server: RpcServerConfig,

    #[clap(flatten)]
    pub storage: StorageConfig,

    #[clap(flatten)]
    pub executor: ExecutorConfig,

    #[clap(flatten)]
    pub miner: MinerConfig,

    #[deref]
    #[clap(flatten)]
    pub common: CommonConfig,

    #[clap(flatten)]
    pub importer: Option<ImporterConfig>,

    #[clap(flatten)]
    #[serde(rename = "kafka")]
    pub kafka_config: Option<KafkaConfig>,
}

impl WithCommonConfig for StratusConfig {
    fn common(&self) -> &CommonConfig {
        &self.common
    }
}

impl StratusConfig {
    /// Validates configuration invariants that cannot be enforced by clap or serde alone,
    /// because values may come from the config file, the CLI, or both.
    pub fn validate(&self) -> anyhow::Result<()> {
        // node mode: exactly one
        let modes = [(self.leader, "leader"), (self.follower, "follower"), (self.fake_leader, "fake-leader")];
        let active: Vec<&str> = modes.iter().filter(|(active, _)| *active).map(|(_, name)| *name).collect();
        match active.as_slice() {
            [_mode] => {}
            [] => anyhow::bail!("no node mode configured: set exactly one of `leader`, `follower` or `fake_leader` (config file or CLI flag)"),
            many => anyhow::bail!(
                "multiple node modes configured ({}): use exactly one of `leader`, `follower`, `fake_leader`",
                many.join(", ")
            ),
        }

        // chain id
        if self.executor.executor_chain_id == 0 {
            anyhow::bail!("`executor.chain_id` is required: set it in the config file or pass `--executor-chain-id`");
        }

        // importer requirements
        if self.leader && self.importer.is_some() {
            anyhow::bail!("leader mode cannot be used with `[importer]` configuration");
        }
        if self.follower || self.fake_leader {
            let Some(importer) = &self.importer else {
                anyhow::bail!("follower and fake-leader modes require `[importer]` configuration");
            };
            if importer.external_rpc.is_empty() {
                anyhow::bail!("`importer.external_rpc` is required for follower and fake-leader modes");
            }
        }

        // kafka: all-or-none
        if let Some(kafka) = &self.kafka_config {
            let missing = [
                ("bootstrap_servers", kafka.bootstrap_servers.is_empty()),
                ("topic", kafka.topic.is_empty()),
                ("client_id", kafka.client_id.is_empty()),
            ];
            let missing: Vec<&str> = missing.iter().filter(|(_, missing)| *missing).map(|(name, _)| *name).collect();
            if !missing.is_empty() {
                anyhow::bail!(
                    "incomplete `[kafka]` configuration: `bootstrap_servers`, `topic` and `client_id` are all required (missing: {})",
                    missing.join(", ")
                );
            }
        }

        // rpc response size floor (same rule as the CLI value parser)
        if self.rpc_server.rpc_max_response_size_bytes < crate::eth::rpc::pagination::MIN_RESPONSE_SIZE_BYTES {
            anyhow::bail!(
                "`rpc.max_response_size_bytes` must be at least {} bytes, otherwise importer pagination cannot fit a chunk",
                crate::eth::rpc::pagination::MIN_RESPONSE_SIZE_BYTES
            );
        }

        // sentry url non-empty when section present
        if self.common.sentry.as_ref().is_some_and(|sentry| sentry.sentry_url.is_empty()) {
            anyhow::bail!("`[sentry]` configuration requires a non-empty `url`");
        }

        // tracing filter: reject invalid directives strings, otherwise they are silently dropped by `EnvFilter`
        if let Err(error) = self.common.tracing.validate_filter() {
            anyhow::bail!("`[common.tracing] filter` is invalid: {error}");
        }

        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Enum: Env
// -----------------------------------------------------------------------------
#[derive(DebugAsJson, strum::Display, strum::VariantNames, Clone, Copy, PartialEq, Eq, Parser, serde::Deserialize, serde::Serialize)]
pub enum Environment {
    #[serde(rename = "local")]
    #[strum(to_string = "local")]
    Local,

    #[serde(rename = "staging")]
    #[strum(to_string = "staging")]
    Staging,

    #[serde(rename = "production")]
    #[strum(to_string = "production")]
    Production,

    #[serde(rename = "canary")]
    #[strum(to_string = "canary")]
    Canary,
}

impl FromStr for Environment {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self, Self::Err> {
        let s = s.trim().to_lowercase();
        match s.as_ref() {
            "local" => Ok(Self::Local),
            "staging" | "test" => Ok(Self::Staging),
            "production" | "prod" => Ok(Self::Production),
            "canary" => Ok(Self::Canary),
            s => Err(anyhow!("unknown environment: \"{}\" - valid values are {:?}", s, Self::VARIANTS)),
        }
    }
}

/// Genesis configuration
#[derive(DebugAsJson, Clone, Parser, Default, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "dev", derive(CliOverrides))]
#[serde(deny_unknown_fields)]
pub struct GenesisFileConfig {
    /// Path to the genesis.json file
    #[arg(long = "genesis-path")]
    #[serde(rename = "path")]
    pub genesis_path: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eth::miner::MinerMode;

    #[test]
    fn test_empty_config_uses_defaults() {
        let config: StratusConfig = toml::from_str("").unwrap();
        let default = StratusConfig::default();
        assert_eq!(serde_json::to_value(&config).unwrap(), serde_json::to_value(&default).unwrap());
    }

    #[test]
    fn test_full_config_parsing() {
        let content = r#"
            follower = true

            [common]
            env = "production"
            async_threads = 8
            blocking_threads = 64
            unknown_client_enabled = false
            blocked_clients = ["metamask", "blockscout"]

            [common.tracing]
            url = "http://collector:4317"
            protocol = "http-json"
            headers = ["key=value"]
            log_format = "json"
            filter = "debug"

            [common.sentry]
            url = "https://sentry.io/123"

            [common.metrics]
            exporter_address = "0.0.0.0:9001"

            [rpc]
            address = "0.0.0.0:3001"
            max_connections = 100
            max_response_size_bytes = 20971520
            max_subscriptions = 10
            health_check_interval_ms = 200
            batch_request_limit = 50
            debug_trace_unsuccessful_only = ["blockscout"]

            [executor]
            chain_id = 100
            call_present_evms = 1
            call_past_evms = 2
            inspector_evms = 3
            reject_not_contract = false
            evm_spec = "Cancun"

            [miner]
            block_mode = "1s"

            [storage.cache]
            account_history_cache_capacity = 30000
            slot_history_cache_capacity = 400000

            [storage.permanent]
            path_prefix = "temp_3001"
            shutdown_timeout = "1m"
            disable_sync_write = true
            cf_size_metrics_interval = "30s"
            file_descriptors_limit = 1024

            [storage.permanent.cf_cache]
            accounts = 1000
            accounts_history = 2000
            account_slots = 3000
            account_slots_history = 4000
            transactions = 5000
            blocks_by_number = 6000
            blocks_by_hash = 7000
            blocks_by_timestamp = 8000
            block_changes = 9000

            {GENESIS_SECTION}

            [importer]
            external_rpc = "http://localhost:3000/"
            external_rpc_ws = "ws://localhost:3000/"
            external_rpc_timeout = "5s"
            sync_interval = "250ms"
            enable_block_changes_replication = true
            forward_access_list = false
            stop_at_block = "0x2a"

            [kafka]
            bootstrap_servers = "localhost:29092"
            topic = "stratus-events"
            client_id = "stratus-producer"
            group_id = "stratus-group"
            security_protocol = "sasl-ssl"
            sasl_mechanisms = "plain"
            sasl_username = "user"
            sasl_password = "pass"
            ssl_ca_location = "/ca.pem"
            ssl_certificate_location = "/cert.pem"
            ssl_key_location = "/key.pem"
        "#;

        #[cfg(feature = "dev")]
        const GENESIS_SECTION: &str = "[storage.permanent.genesis]\n            path = \"config/genesis.local.json\"";
        #[cfg(not(feature = "dev"))]
        const GENESIS_SECTION: &str = "";

        let content = content.replace("{GENESIS_SECTION}", GENESIS_SECTION);
        let config: StratusConfig = toml::from_str(&content).unwrap();

        assert!(!config.leader);
        assert!(config.follower);
        assert_eq!(config.common.env, Environment::Production);
        assert_eq!(config.common.num_async_threads, 8);
        assert_eq!(config.common.num_blocking_threads, 64);
        assert!(!config.common.unknown_client_enabled);
        assert_eq!(config.common.blocked_clients, ["metamask", "blockscout"]);
        assert_eq!(config.common.tracing.tracing_url.as_deref(), Some("http://collector:4317"));
        assert_eq!(config.common.tracing.tracing_log_format.to_string(), "json");
        assert_eq!(config.common.tracing.tracing_filter.as_deref(), Some("debug"));
        assert_eq!(config.common.sentry.as_ref().unwrap().sentry_url, "https://sentry.io/123");
        assert_eq!(config.common.metrics.metrics_exporter_address.to_string(), "0.0.0.0:9001");
        assert_eq!(config.rpc_server.rpc_address.to_string(), "0.0.0.0:3001");
        assert_eq!(config.rpc_server.rpc_max_connections, 100);
        assert_eq!(config.rpc_server.rpc_max_response_size_bytes, 20971520);
        assert_eq!(config.rpc_server.rpc_debug_trace_unsuccessful_only.as_ref().unwrap().len(), 1);
        assert_eq!(config.executor.executor_chain_id, 100);
        assert_eq!(config.executor.call_present_evms, 1);
        assert!(!config.executor.executor_reject_not_contract);
        assert_eq!(config.executor.executor_evm_spec.to_string(), "Cancun");
        assert_eq!(config.miner.block_mode, MinerMode::Interval(std::time::Duration::from_secs(1)));
        assert_eq!(config.storage.cache.account_history_cache_capacity, 30000);
        assert_eq!(config.storage.perm_storage.rocks_path_prefix.as_deref(), Some("temp_3001"));
        assert_eq!(config.storage.perm_storage.rocks_shutdown_timeout, std::time::Duration::from_secs(60));
        assert!(config.storage.perm_storage.rocks_disable_sync_write);
        assert_eq!(
            config.storage.perm_storage.rocks_cf_size_metrics_interval,
            Some(std::time::Duration::from_secs(30))
        );
        assert_eq!(config.storage.perm_storage.rocks_cf_cache.accounts, 1000);
        #[cfg(feature = "dev")]
        assert_eq!(
            config.storage.perm_storage.genesis_file.genesis_path.as_deref(),
            Some("config/genesis.local.json")
        );
        assert_eq!(config.importer.as_ref().unwrap().external_rpc, "http://localhost:3000/");
        assert_eq!(config.importer.as_ref().unwrap().external_rpc_timeout, std::time::Duration::from_secs(5));
        assert_eq!(config.importer.as_ref().unwrap().sync_interval, std::time::Duration::from_millis(250));
        assert!(config.importer.as_ref().unwrap().enable_block_changes_replication);
        assert!(!config.importer.as_ref().unwrap().forward_access_list);
        assert_eq!(
            config.importer.as_ref().unwrap().stop_at_block,
            Some(crate::eth::types::BlockNumber::from(42u64))
        );
        let kafka = config.kafka_config.as_ref().unwrap();
        assert_eq!(kafka.bootstrap_servers, "localhost:29092");
        assert_eq!(kafka.topic, "stratus-events");

        // the full example must pass validation
        config.validate().unwrap();
    }

    #[test]
    fn test_unknown_fields_are_rejected() {
        let content = r#"
            [exector]
            chain_id = 100
        "#;
        let error = toml::from_str::<StratusConfig>(content).unwrap_err();
        assert!(error.to_string().contains("unknown field"), "unexpected error: {error}");
    }

    #[test]
    fn test_validate_requires_node_mode() {
        let config = StratusConfig {
            executor: ExecutorConfig {
                executor_chain_id: 100,
                ..Default::default()
            },
            ..Default::default()
        };
        let error = config.validate().unwrap_err();
        assert!(error.to_string().contains("no node mode"), "unexpected error: {error}");
    }

    #[test]
    fn test_validate_requires_chain_id() {
        let config = StratusConfig {
            leader: true,
            ..Default::default()
        };
        let error = config.validate().unwrap_err();
        assert!(error.to_string().contains("chain_id"), "unexpected error: {error}");
        let config = StratusConfig {
            leader: true,
            executor: ExecutorConfig {
                executor_chain_id: 2008,
                ..Default::default()
            },
            ..Default::default()
        };
        config.validate().unwrap();
    }

    #[test]
    fn test_validate_rejects_multiple_node_modes() {
        let config = StratusConfig {
            leader: true,
            follower: true,
            ..Default::default()
        };
        let error = config.validate().unwrap_err();
        assert!(error.to_string().contains("multiple node modes"), "unexpected error: {error}");
    }

    #[test]
    fn test_validate_rejects_invalid_tracing_filter() {
        let mut config = StratusConfig {
            leader: true,
            executor: ExecutorConfig {
                executor_chain_id: 2008,
                ..Default::default()
            },
            ..Default::default()
        };
        config.common.tracing.tracing_filter = Some("stratus=bogus-level".to_string());
        let error = config.validate().unwrap_err();
        assert!(error.to_string().contains("filter"), "unexpected error: {error}");

        config.common.tracing.tracing_filter = Some("info,stratus::eth=debug,jsonrpsee-server=off".to_string());
        config.validate().unwrap();
    }

    #[test]
    fn test_validate_requires_importer_for_follower() {
        let config = StratusConfig {
            follower: true,
            executor: ExecutorConfig {
                executor_chain_id: 2008,
                ..Default::default()
            },
            ..Default::default()
        };
        let error = config.validate().unwrap_err();
        assert!(error.to_string().contains("importer"), "unexpected error: {error}");

        let config = StratusConfig {
            follower: true,
            executor: ExecutorConfig {
                executor_chain_id: 2008,
                ..Default::default()
            },
            importer: Some(ImporterConfig::default()),
            ..Default::default()
        };
        let error = config.validate().unwrap_err();
        assert!(error.to_string().contains("external_rpc"), "unexpected error: {error}");
    }

    #[test]
    fn test_validate_rejects_incomplete_kafka() {
        let config = StratusConfig {
            leader: true,
            executor: ExecutorConfig {
                executor_chain_id: 2008,
                ..Default::default()
            },
            kafka_config: Some(KafkaConfig::default()),
            ..Default::default()
        };
        let error = config.validate().unwrap_err();
        assert!(error.to_string().contains("kafka"), "unexpected error: {error}");

        let kafka = KafkaConfig {
            bootstrap_servers: "localhost:29092".to_string(),
            topic: "stratus-events".to_string(),
            client_id: "stratus-producer".to_string(),
            ..Default::default()
        };
        let config = StratusConfig {
            leader: true,
            executor: ExecutorConfig {
                executor_chain_id: 2008,
                ..Default::default()
            },
            kafka_config: Some(kafka),
            ..Default::default()
        };
        config.validate().unwrap();
    }
}
