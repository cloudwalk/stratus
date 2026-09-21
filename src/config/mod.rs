//! Application configuration.
//!
//! Configuration is loaded from a TOML file and can be overridden by explicitly provided CLI arguments.
//! See [`crate::config::loader`] for the loading rules.

pub mod loader;
mod validate;

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
pub use validate::Validation;

use crate::eth::executor::ExecutorConfig;
use crate::eth::follower::importer::ImporterConfig;
use crate::eth::miner::MinerConfig;
use crate::eth::rpc::ExporterConfig;
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

#[cfg(test)]
impl Default for CommonConfig {
    fn default() -> Self {
        Self {
            env: Environment::Local,
            num_async_threads: 32,
            num_blocking_threads: 512,
            tracing: TracingConfig::default(),
            sentry: None,
            metrics: MetricsConfig {
                metrics_exporter_address: std::net::SocketAddr::from(([0, 0, 0, 0], 9000)),
            },
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
#[derive(DebugAsJson, Clone, Parser, derive_more::Deref, serde::Serialize)]
#[cfg_attr(test, derive(Default))]
#[clap(group = ArgGroup::new("mode").args(&["leader", "follower", "fake_leader"]).required(true))]
pub struct StratusConfig {
    #[arg(id = "leader", long = "leader", conflicts_with_all = ["follower", "fake_leader"])]
    pub leader: bool,

    #[arg(id = "follower", long = "follower", conflicts_with_all = ["leader", "fake_leader"], requires = "importer.external_rpc")]
    pub follower: bool,

    /// The fake leader imports blocks like a follower, but executes the blocks's txs locally like a leader.
    #[arg(id = "fake_leader", long = "fake-leader", conflicts_with_all = ["leader", "follower"], requires = "importer.external_rpc")]
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

    #[clap(flatten)]
    pub exporter: ExporterConfig,

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
    pub(crate) fn ignore_sentry_without_url(&mut self) {
        if self.common.sentry.as_ref().is_some_and(|sentry| sentry.sentry_url.is_empty()) {
            println!("warning: ignoring [common.sentry] config: url is empty");
            self.common.sentry = None;
        }
    }

    /// Returns the names of the active node modes.
    fn active_node_modes(&self) -> Vec<&'static str> {
        [(self.leader, "leader"), (self.follower, "follower"), (self.fake_leader, "fake-leader")]
            .into_iter()
            .filter(|(active, _)| *active)
            .map(|(_, name)| name)
            .collect()
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
#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
#[cfg_attr(test, derive(Default))]
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
            "--executor-chain-id",
            "2008",
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
            "--executor-chain-id",
            "2008",
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
