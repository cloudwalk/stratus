//! Application configuration.

use std::env;
use std::str::FromStr;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::anyhow;
use clap::ArgGroup;
use clap::Parser;
use display_json::DebugAsJson;
use strum::VariantNames;
use tokio::runtime::Builder;
use tokio::runtime::Runtime;

use crate::eth::executor::ExecutorConfig;
use crate::eth::external_rpc::ExternalRpcConfig;
use crate::eth::follower::importer::ImporterConfig;
use crate::eth::miner::MinerConfig;
use crate::eth::primitives::Address;
use crate::eth::rpc::RpcServerConfig;
use crate::eth::storage::StorageConfig;
use crate::ext::parse_duration;
use crate::infra::build_info;
use crate::infra::kafka::KafkaConfig;
use crate::infra::metrics::MetricsConfig;
use crate::infra::sentry::SentryConfig;
use crate::infra::tracing::TracingConfig;

/// Loads .env files according to the binary and environment.
pub fn load_dotenv_file() {
    // parse env manually because this is executed before clap
    let env = match std::env::var("ENV") {
        Ok(env) => Environment::from_str(env.as_str()),
        Err(_) => Ok(Environment::Local),
    };

    // determine the .env file to load
    let env_filename = match env {
        Ok(Environment::Local) => {
            // local environment only
            match std::env::var("LOCAL_ENV_PATH") {
                Ok(local_path) => local_path,
                Err(_) => format!("config/{}.env.local", build_info::binary_name()),
            }
        }
        Ok(env) => format!("config/{}.env.{}", build_info::binary_name(), env),
        Err(e) => {
            println!("{e}");
            return;
        }
    };

    println!("reading env file | filename={}", env_filename);

    if let Err(e) = dotenvy::from_filename(env_filename) {
        println!("env file error: {e}");
    }
}

/// Applies env-var aliases because Clap does not support this feature.
pub fn load_env_aliases() {
    fn env_alias(canonical: &'static str, alias: &'static str) {
        if let Ok(value) = env::var(alias) {
            env::set_var(canonical, value);
        }
    }
    env_alias("EXECUTOR_CHAIN_ID", "CHAIN_ID");
    env_alias("EXECUTOR_EVMS", "EVMS");
    env_alias("EXECUTOR_EVMS", "NUM_EVMS");
    env_alias("EXECUTOR_REJECT_NOT_CONTRACT", "REJECT_NOT_CONTRACT");
    env_alias("EXECUTOR_STRATEGY", "STRATEGY");
    env_alias("TRACING_LOG_FORMAT", "LOG_FORMAT");
    env_alias("TRACING_URL", "TRACING_COLLECTOR_URL");
}

// -----------------------------------------------------------------------------
// Config: Common
// -----------------------------------------------------------------------------

pub trait WithCommonConfig {
    fn common(&self) -> &CommonConfig;
}

/// Configuration that can be used by any binary.
#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
#[command(author, version, about, long_about = None)]
pub struct CommonConfig {
    /// Environment where the application is running.
    #[arg(long = "env", env = "ENV", default_value = "local")]
    pub env: Environment,

    /// Number of threads to execute global async tasks.
    #[arg(long = "async-threads", env = "ASYNC_THREADS", default_value = "32")]
    pub num_async_threads: usize,

    /// Number of threads to execute global blocking tasks.
    #[arg(long = "blocking-threads", env = "BLOCKING_THREADS", default_value = "512")]
    pub num_blocking_threads: usize,

    #[clap(flatten)]
    pub tracing: TracingConfig,

    #[clap(flatten)]
    pub sentry: Option<SentryConfig>,

    #[clap(flatten)]
    pub metrics: MetricsConfig,

    /// Prevents clap from breaking when passing `nocapture` options in tests.
    #[arg(long = "nocapture")]
    pub nocapture: bool,

    /// Enables or disables unknown client interactions.
    #[arg(long = "unknown-client-enabled", env = "UNKNOWN_CLIENT_ENABLED", default_value = "true")]
    pub unknown_client_enabled: bool,
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
                        return format!("tokio-async");
                    } else {
                        return format!("tokio-async-{}", async_id);
                    }
                }

                // identify blocking threads
                let blocking_id = BLOCKING_ID.fetch_add(1, Ordering::SeqCst);
                if cfg!(feature = "flamegraph") {
                    format!("tokio-blocking")
                } else {
                    format!("tokio-blocking-{}", blocking_id)
                }
            })
            .build();

        match result {
            Ok(runtime) => Ok(runtime),
            Err(e) => {
                println!("failed to create tokio runtime | reason={:?}", e);
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
#[clap(group = ArgGroup::new("mode").required(true).args(&["leader", "follower", "fake_leader"]))]
pub struct StratusConfig {
    #[arg(long = "leader", env = "LEADER", conflicts_with_all = ["follower", "fake_leader", "ImporterConfig"])]
    pub leader: bool,

    #[arg(long = "follower", env = "FOLLOWER", conflicts_with_all = ["leader", "fake_leader"], requires = "ImporterConfig")]
    pub follower: bool,

    /// The fake leader imports blocks like a follower, but executes the blocks's txs locally like a leader.
    #[arg(long = "fake-leader", env = "FAKE_LEADER", conflicts_with_all = ["leader", "follower"], requires = "ImporterConfig")]
    pub fake_leader: bool,

    #[clap(flatten)]
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
    pub kafka_config: Option<KafkaConfig>,
}

impl WithCommonConfig for StratusConfig {
    fn common(&self) -> &CommonConfig {
        &self.common
    }
}

// -----------------------------------------------------------------------------
// Config: RpcDownloader
// -----------------------------------------------------------------------------

/// Configuration for `rpc-downlaoder` binary.
#[derive(DebugAsJson, Clone, Parser, derive_more::Deref, serde::Serialize)]
pub struct RpcDownloaderConfig {
    /// Final block number to be downloaded.
    #[arg(long = "block-end", env = "BLOCK_END")]
    pub block_end: Option<u64>,

    #[clap(flatten)]
    pub rpc_storage: ExternalRpcConfig,

    /// External RPC endpoint to sync blocks with Stratus.
    #[arg(short = 'r', long = "external-rpc", env = "EXTERNAL_RPC")]
    pub external_rpc: String,

    /// Timeout for blockchain requests
    #[arg(long = "external-rpc-timeout", value_parser=parse_duration, env = "EXTERNAL_RPC_TIMEOUT", default_value = "2s")]
    pub external_rpc_timeout: Duration,

    /// Number of parallel downloads.
    #[arg(short = 'p', long = "paralellism", env = "PARALELLISM", default_value = "1")]
    pub paralellism: usize,

    /// Accounts to retrieve initial balance information.
    ///
    /// For Cloudwalk networks, provide these addresses:
    /// - Mainnet: 0xF56A88A4afF45cdb5ED7Fe63a8b71aEAaFF24FA6
    /// - Testnet: 0xE45b176cAd7090A5CF70B69a73b6DEF9296ba6A2
    #[arg(long = "initial-accounts", env = "INITIAL_ACCOUNTS", value_delimiter = ',')]
    pub initial_accounts: Vec<Address>,

    #[deref]
    #[clap(flatten)]
    pub common: CommonConfig,
}

impl WithCommonConfig for RpcDownloaderConfig {
    fn common(&self) -> &CommonConfig {
        &self.common
    }
}

// -----------------------------------------------------------------------------
// Config: ImporterOffline
// -----------------------------------------------------------------------------

/// Configuration for `importer-offline` binary.
#[derive(Parser, DebugAsJson, derive_more::Deref, serde::Serialize)]
pub struct ImporterOfflineConfig {
    /// Initial block number to be imported.
    #[arg(long = "block-start", env = "BLOCK_START")]
    pub block_start: Option<u64>,

    /// Final block number to be imported.
    #[arg(long = "block-end", env = "BLOCK_END")]
    pub block_end: Option<u64>,

    /// Number of parallel database fetches.
    #[arg(short = 'p', long = "paralellism", env = "PARALELLISM", default_value = "1")]
    pub paralellism: usize,

    /// Number of blocks by database fetch.
    #[arg(short = 'b', long = "blocks-by-fetch", env = "BLOCKS_BY_FETCH", default_value = "10000")]
    pub blocks_by_fetch: usize,

    #[clap(flatten)]
    pub executor: ExecutorConfig,

    #[clap(flatten)]
    pub miner: MinerConfig,

    #[clap(flatten)]
    pub storage: StorageConfig,

    #[clap(flatten)]
    pub rpc_storage: ExternalRpcConfig,

    #[deref]
    #[clap(flatten)]
    pub common: CommonConfig,
}

impl WithCommonConfig for ImporterOfflineConfig {
    fn common(&self) -> &CommonConfig {
        &self.common
    }
}

// -----------------------------------------------------------------------------
// Config: RocksRevertToBlockConfig
// -----------------------------------------------------------------------------

#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
pub struct RocksRevertToBlockConfig {
    /// Block number to revert to.
    #[arg(long = "block", env = "BLOCK")]
    pub block_number: u64,

    #[arg(long = "rocks-path-prefix", env = "ROCKS_PATH_PREFIX")]
    pub rocks_path_prefix: Option<String>,

    #[clap(flatten)]
    pub common: CommonConfig,
}

impl WithCommonConfig for RocksRevertToBlockConfig {
    fn common(&self) -> &CommonConfig {
        &self.common
    }
}

// -----------------------------------------------------------------------------
// Config: Test
// -----------------------------------------------------------------------------

/// Configuration for integration tests.
#[derive(DebugAsJson, Clone, Parser, derive_more::Deref, serde::Serialize)]
pub struct IntegrationTestConfig {
    #[deref]
    #[clap(flatten)]
    pub common: CommonConfig,

    #[clap(flatten)]
    pub executor: ExecutorConfig,

    #[clap(flatten)]
    pub miner: MinerConfig,

    #[clap(flatten)]
    pub storage: StorageConfig,

    #[clap(flatten)]
    pub rpc_storage: ExternalRpcConfig,
}

impl WithCommonConfig for IntegrationTestConfig {
    fn common(&self) -> &CommonConfig {
        &self.common
    }
}

// -----------------------------------------------------------------------------
// Enum: Env
// -----------------------------------------------------------------------------
#[derive(DebugAsJson, strum::Display, strum::VariantNames, Clone, Copy, Parser, serde::Serialize)]
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

// -----------------------------------------------------------------------------
// Enum: ValidatorMethodConfig
// -----------------------------------------------------------------------------

#[derive(DebugAsJson, Clone, strum::Display, serde::Serialize)]
pub enum ValidatorMethodConfig {
    Rpc { url: String },
    CompareTables,
}

impl FromStr for ValidatorMethodConfig {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self, Self::Err> {
        match s {
            "compare_tables" => Ok(Self::CompareTables),
            s => Ok(Self::Rpc { url: s.to_string() }),
        }
    }
}
