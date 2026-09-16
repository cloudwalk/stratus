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
use stratus_metrics::MetricsConfig;
use strum::VariantNames;
use tokio::runtime::Builder;
use tokio::runtime::Runtime;

use crate::eth::executor::ExecutorConfig;
use crate::eth::follower::importer::ImporterConfig;
use crate::eth::miner::MinerConfig;
use crate::eth::rpc::RpcServerConfig;
use crate::eth::storage::StorageConfig;
use crate::infra::kafka::KafkaConfig;
use crate::infra::sentry::SentryConfig;
use crate::infra::tracing::TracingConfig;

// -----------------------------------------------------------------------------
// Config: Common
// -----------------------------------------------------------------------------

pub trait WithCommonConfig {
    fn common(&self) -> &CommonConfig;
}

/// Configuration that can be used by any binary.
///
/// Argument ids are the dotted TOML paths of the corresponding config file fields; the loader
/// uses them to apply file values as clap defaults (see [`loader`]).
#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
pub struct CommonConfig {
    /// Environment where the application is running.
    #[arg(id = "common.env", long = "env", default_value = "local")]
    pub env: Environment,

    /// Number of threads to execute global async tasks.
    #[arg(id = "common.async_threads", long = "async-threads", default_value = "32")]
    #[serde(rename = "async_threads")]
    pub num_async_threads: usize,

    /// Number of threads to execute global blocking tasks.
    #[arg(id = "common.blocking_threads", long = "blocking-threads", default_value = "512")]
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
    #[arg(
        id = "common.unknown_client_enabled",
        long = "unknown-client-enabled",
        default_value = "true",
        default_missing_value = "true",
        action = clap::ArgAction::Set,
        num_args = 0..=1
    )]
    pub unknown_client_enabled: bool,

    /// Comma-separated list of client names that are blocked from interacting with the application.
    /// Client names are matched the same way as the `app`/`client` identification headers/params.
    #[arg(id = "common.blocked_clients", long = "blocked-clients", value_delimiter = ',')]
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
#[derive(DebugAsJson, Clone, Default, Parser, derive_more::Deref, serde::Serialize)]
#[clap(group = ArgGroup::new("mode").args(&["leader", "follower", "fake_leader"]).required(true))]
pub struct StratusConfig {
    #[arg(id = "leader", long = "leader", conflicts_with_all = ["follower", "fake_leader"])]
    pub leader: bool,

    #[arg(id = "follower", long = "follower", conflicts_with_all = ["leader", "fake_leader"])]
    pub follower: bool,

    /// The fake leader imports blocks like a follower, but executes the blocks's txs locally like a leader.
    #[arg(id = "fake_leader", long = "fake-leader", conflicts_with_all = ["leader", "follower"])]
    pub fake_leader: bool,

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
    /// Ignores follower-only sections when running as leader.
    ///
    /// `[importer]` and `[kafka]` only apply to follower and fake-leader nodes; a leader receiving them
    /// (e.g. from a config file shared with a follower) ignores them instead of failing to start.
    pub(crate) fn ignore_follower_sections(&mut self) {
        if self.active_node_modes().as_slice() != ["leader"] {
            return;
        }
        if self.importer.take().is_some() {
            println!("warning: ignoring [importer] config in leader mode");
        }
        if self.kafka_config.take().is_some() {
            println!("warning: ignoring [kafka] config in leader mode");
        }
    }

    /// Ignores the sentry section when its url is empty.
    ///
    /// Sentry is non-essential: an empty url disables the exporter instead of failing to start,
    /// the same way a sentry exporter that fails to start is handled at runtime.
    pub(crate) fn ignore_sentry_without_url(&mut self) {
        if self.common.sentry.as_ref().is_some_and(|sentry| sentry.sentry_url.is_empty()) {
            println!("warning: ignoring [common.sentry] config: url is empty");
            self.common.sentry = None;
        }
    }

    /// Validates configuration invariants that clap cannot enforce.
    ///
    /// Clap's value parsers already validate per-value syntax and ranges for file and CLI values, and the node mode
    /// is enforced by clap itself: a required group guarantees at least one mode, and the conflicts between the
    /// flags guarantee at most one. What is left are relations clap cannot express: `executor.chain_id` uses `0` as
    /// its clap default (so a value parser cannot reject it), the importer requirements for follower and
    /// fake-leader modes, and the all-or-none kafka section.
    pub fn validate(&self) -> anyhow::Result<()> {
        self.validate_executor()?;
        self.validate_importer()?;
        self.validate_kafka()?;
        Ok(())
    }

    /// Returns the names of the active node modes.
    fn active_node_modes(&self) -> Vec<&'static str> {
        [(self.leader, "leader"), (self.follower, "follower"), (self.fake_leader, "fake-leader")]
            .into_iter()
            .filter(|(active, _)| *active)
            .map(|(_, name)| name)
            .collect()
    }

    /// Validates that a chain id is configured.
    fn validate_executor(&self) -> anyhow::Result<()> {
        if self.executor.executor_chain_id == 0 {
            anyhow::bail!("`executor.chain_id` is required: set it in the config file or pass `--executor-chain-id`");
        }
        Ok(())
    }

    /// Validates the importer requirements for follower and fake-leader modes.
    fn validate_importer(&self) -> anyhow::Result<()> {
        if self.follower || self.fake_leader {
            let Some(importer) = &self.importer else {
                anyhow::bail!("follower and fake-leader modes require `[importer]` configuration");
            };
            if importer.external_rpc.is_empty() {
                anyhow::bail!("`importer.external_rpc` is required for follower and fake-leader modes");
            }
        }
        Ok(())
    }

    /// Validates the kafka section: all-or-none fields, for follower and fake-leader modes.
    fn validate_kafka(&self) -> anyhow::Result<()> {
        let Some(kafka) = &self.kafka_config else { return Ok(()) };
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
#[derive(DebugAsJson, Clone, Parser, Default, serde::Serialize)]
pub struct GenesisFileConfig {
    /// Path to the genesis.json file
    #[arg(id = "storage.permanent.genesis.path", long = "genesis-path")]
    #[serde(rename = "path")]
    pub genesis_path: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_true_flags_accept_explicit_false() {
        // these bools default to `true`; the bare flag and the `=false`/`=true` forms must all work
        let config = StratusConfig::try_parse_from([
            "stratus",
            "--leader",
            "--executor-reject-not-contract=false",
            "--unknown-client-enabled=false",
            "--forward-access-list=false",
            "-r",
            "http://localhost:3000/",
        ])
        .unwrap();
        assert!(!config.executor.executor_reject_not_contract);
        assert!(!config.common.unknown_client_enabled);
        assert!(!config.importer.as_ref().unwrap().forward_access_list);

        let config = StratusConfig::try_parse_from([
            "stratus",
            "--leader",
            "--executor-reject-not-contract",
            "--unknown-client-enabled",
            "--forward-access-list",
            "-r",
            "http://localhost:3000/",
        ])
        .unwrap();
        assert!(config.executor.executor_reject_not_contract);
        assert!(config.common.unknown_client_enabled);
        assert!(config.importer.as_ref().unwrap().forward_access_list);
    }
}
