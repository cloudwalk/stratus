//! Application configuration.

use std::any::Any;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::anyhow;
use clap::Parser;
use display_json::DebugAsJson;
use strum::VariantNames;
use tokio::runtime::Builder;
use tokio::runtime::Runtime;

use crate::eth::executor::ExecutorConfig;
use crate::eth::importer::ImporterConfig;
use crate::eth::miner::MinerConfig;
use crate::eth::primitives::Address;
use crate::eth::rpc::RpcServerConfig;
use crate::eth::storage::ExternalRpcStorageConfig;
use crate::eth::storage::StratusStorageConfig;
use crate::ext::parse_duration;
use crate::infra::build_info;
use crate::infra::tracing::TracingConfig;

/// Loads .env files according to the binary and environment.
pub fn load_dotenv() {
    // parse env manually because this is executed before clap
    let env = match std::env::var("ENV") {
        Ok(env) => Environment::from_str(env.as_str()),
        Err(_) => Ok(Environment::Local),
    };
    let env = match env {
        Ok(env) => env,
        Err(e) => {
            println!("{e}");
            return;
        }
    };

    // load .env file
    let env_filename = format!("config/{}.env.{}", build_info::binary_name(), env);
    println!("reading env file | filename={}", env_filename);

    if let Err(e) = dotenvy::from_filename(env_filename) {
        println!("env file error: {e}");
    }
}

// -----------------------------------------------------------------------------
// Config: Common
// -----------------------------------------------------------------------------

pub trait WithCommonConfig {
    fn common(&self) -> &CommonConfig;
    fn as_any(&self) -> &dyn Any;
}

/// Configuration that can be used by any binary.
#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
#[command(author, version, about, long_about = None)]
pub struct CommonConfig {
    /// Environment where the application is running.
    #[arg(long = "env", env = "ENV", default_value = "local")]
    pub env: Environment,

    /// Stratus mode.
    #[arg(long = "mode", env = "MODE")]
    pub mode: StratusMode,

    /// Number of threads to execute global async tasks.
    #[arg(long = "async-threads", env = "ASYNC_THREADS", default_value = "10")]
    pub num_async_threads: usize,

    /// Number of threads to execute global blocking tasks.
    #[arg(long = "blocking-threads", env = "BLOCKING_THREADS", default_value = "10")]
    pub num_blocking_threads: usize,

    #[clap(flatten)]
    pub tracing: TracingConfig,

    /// Address where Prometheus metrics will be exposed.
    #[arg(long = "metrics-exporter-address", env = "METRICS_EXPORTER_ADDRESS", default_value = "0.0.0.0:9000")]
    pub metrics_exporter_address: SocketAddr,

    // Address where Tokio Console GRPC server will be exposed.
    #[arg(long = "tokio-console-address", env = "TRACING_TOKIO_CONSOLE_ADDRESS")]
    pub tokio_console_address: Option<SocketAddr>,

    /// Sentry URL where error events will be pushed.
    #[arg(long = "sentry-url", env = "SENTRY_URL")]
    pub sentry_url: Option<String>,

    /// Direct access to peers via IP address, why will be included on data propagation and leader election.
    #[arg(long = "candidate-peers", env = "CANDIDATE_PEERS", value_delimiter = ',')]
    pub candidate_peers: Vec<String>,

    // Address for the GRPC Server
    #[arg(long = "grpc-server-address", env = "GRPC_SERVER_ADDRESS", default_value = "0.0.0.0:3777")]
    pub grpc_server_address: SocketAddr,

    /// Prevents clap from breaking when passing `nocapture` options in tests.
    #[arg(long = "nocapture")]
    pub nocapture: bool,
}

impl WithCommonConfig for CommonConfig {
    fn common(&self) -> &CommonConfig {
        self
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl CommonConfig {
    /// Initializes Tokio runtime.
    pub fn init_runtime(&self) -> anyhow::Result<Runtime> {
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
                    return format!("tokio-async-{}", async_id);
                }

                // identify blocking threads
                let blocking_id = BLOCKING_ID.fetch_add(1, Ordering::SeqCst);
                format!("tokio-blocking-{}", blocking_id)
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
pub struct StratusConfig {
    #[clap(flatten)]
    pub rpc_server: RpcServerConfig,

    #[clap(flatten)]
    pub storage: StratusStorageConfig,

    #[clap(flatten)]
    pub executor: ExecutorConfig,

    #[clap(flatten)]
    pub miner: MinerConfig,

    #[clap(flatten)]
    pub importer: Option<ImporterConfig>,

    #[deref]
    #[clap(flatten)]
    pub common: CommonConfig,
}

impl WithCommonConfig for StratusConfig {
    fn common(&self) -> &CommonConfig {
        &self.common
    }

    fn as_any(&self) -> &dyn Any {
        self
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
    pub rpc_storage: ExternalRpcStorageConfig,

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

    fn as_any(&self) -> &dyn Any {
        self
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

    /// Export selected blocks to fixtures snapshots to be used in tests.
    #[arg(long = "export-snapshot", env = "EXPORT_SNAPSHOT", value_delimiter = ',')]
    pub export_snapshot: Vec<u64>,

    #[clap(flatten)]
    pub executor: ExecutorConfig,

    #[clap(flatten)]
    pub miner: MinerConfig,

    #[clap(flatten)]
    pub storage: StratusStorageConfig,

    #[clap(flatten)]
    pub rpc_storage: ExternalRpcStorageConfig,

    #[deref]
    #[clap(flatten)]
    pub common: CommonConfig,
}

impl WithCommonConfig for ImporterOfflineConfig {
    fn common(&self) -> &CommonConfig {
        &self.common
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// -----------------------------------------------------------------------------
// Config: ImporterOnline
// -----------------------------------------------------------------------------

/// Configuration for `importer-online` binary.
#[derive(DebugAsJson, Clone, Parser, derive_more::Deref, serde::Serialize)]
pub struct ImporterOnlineConfig {
    #[clap(flatten)]
    pub base: ImporterOnlineBaseConfig,

    #[clap(flatten)]
    pub executor: ExecutorConfig,

    #[clap(flatten)]
    pub miner: MinerConfig,

    #[clap(flatten)]
    pub storage: StratusStorageConfig,

    #[deref]
    #[clap(flatten)]
    pub common: CommonConfig,
}

#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
pub struct ImporterOnlineBaseConfig {
    /// External RPC HTTP endpoint to sync blocks with Stratus.
    #[arg(short = 'r', long = "external-rpc", env = "EXTERNAL_RPC")]
    pub external_rpc: String,

    /// External RPC WS endpoint to sync blocks with Stratus.
    #[arg(short = 'w', long = "external-rpc-ws", env = "EXTERNAL_RPC_WS")]
    pub external_rpc_ws: Option<String>,

    /// Timeout for blockchain requests (importer online)
    #[arg(long = "external-rpc-timeout", value_parser=parse_duration, env = "EXTERNAL_RPC_TIMEOUT", default_value = "2s")]
    pub external_rpc_timeout: Duration,

    #[arg(long = "sync-interval", value_parser=parse_duration, env = "SYNC_INTERVAL", default_value = "100ms")]
    pub sync_interval: Duration,
}

impl WithCommonConfig for ImporterOnlineConfig {
    fn common(&self) -> &CommonConfig {
        &self.common
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(DebugAsJson, Clone, Parser, derive_more::Deref, serde::Serialize)]
pub struct RunWithImporterConfig {
    #[clap(flatten)]
    pub rpc_server: RpcServerConfig,

    #[arg(long = "leader-node", env = "LEADER_NODE")]
    pub leader_node: Option<String>, // to simulate this in use locally with other nodes, you need to add the node name into /etc/hostname

    #[clap(flatten)]
    pub online: ImporterOnlineBaseConfig,

    #[clap(flatten)]
    pub storage: StratusStorageConfig,

    #[clap(flatten)]
    pub executor: ExecutorConfig,

    #[clap(flatten)]
    pub miner: MinerConfig,

    #[deref]
    #[clap(flatten)]
    pub common: CommonConfig,
}

impl WithCommonConfig for RunWithImporterConfig {
    fn common(&self) -> &CommonConfig {
        &self.common
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// -----------------------------------------------------------------------------
// Config: StateValidator
// -----------------------------------------------------------------------------

/// Configuration for `state-validator` binary.
#[derive(DebugAsJson, Clone, Parser, derive_more::Deref, serde::Serialize)]
pub struct StateValidatorConfig {
    /// How many slots to validate per batch. 0 means every slot.
    #[arg(long = "max-samples", env = "MAX_SAMPLES", default_value_t = 0)]
    pub sample_size: u64,

    /// Seed to use when sampling. 0 for random seed.
    #[arg(long = "seed", env = "SEED", default_value_t = 0, requires = "sample_size")]
    pub seed: u64,

    /// Validate in batches of n blocks.
    #[arg(short = 'i', long = "interval", env = "INTERVAL", default_value = "1000")]
    pub interval: u64,

    /// What method to use when validating.
    #[arg(short = 'm', long = "method", env = "METHOD")]
    pub method: ValidatorMethodConfig,

    /// How many concurrent validation tasks to run
    #[arg(short = 'c', long = "concurrent-tasks", env = "CONCURRENT_TASKS", default_value_t = 10)]
    pub concurrent_tasks: u16,

    #[deref]
    #[clap(flatten)]
    pub common: CommonConfig,

    #[clap(flatten)]
    pub storage: StratusStorageConfig,
}

impl WithCommonConfig for StateValidatorConfig {
    fn common(&self) -> &CommonConfig {
        &self.common
    }

    fn as_any(&self) -> &dyn Any {
        self
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
    pub storage: StratusStorageConfig,

    #[clap(flatten)]
    pub rpc_storage: ExternalRpcStorageConfig,
}

impl WithCommonConfig for IntegrationTestConfig {
    fn common(&self) -> &CommonConfig {
        &self.common
    }

    fn as_any(&self) -> &dyn Any {
        self
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
}

impl FromStr for Environment {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self, Self::Err> {
        let s = s.trim().to_lowercase();
        match s.as_ref() {
            "local" => Ok(Self::Local),
            "staging" | "test" => Ok(Self::Staging),
            "production" | "prod" => Ok(Self::Production),
            s => Err(anyhow!("unknown environment: \"{}\" - valid values are {:?}", s, Environment::VARIANTS)),
        }
    }
}

// -----------------------------------------------------------------------------
// Enum: Stratus Mode
// -----------------------------------------------------------------------------
#[derive(DebugAsJson, PartialEq, strum::Display, strum::VariantNames, Clone, Copy, Parser, serde::Serialize)]
pub enum StratusMode {
    #[serde(rename = "leader")]
    #[strum(to_string = "leader")]
    Leader,

    #[serde(rename = "follower")]
    #[strum(to_string = "follower")]
    Follower,
}

impl FromStr for StratusMode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self, Self::Err> {
        let s = s.trim().to_lowercase();
        match s.as_ref() {
            "leader" => Ok(Self::Leader),
            "follower" => Ok(Self::Follower),
            s => Err(anyhow!("unknown stratus mode: \"{}\" - valid values are {:?}", s, StratusMode::VARIANTS)),
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

// -----------------------------------------------------------------------------
// Config: Configuration Checks
// -----------------------------------------------------------------------------

pub trait ConfigChecks {
    fn perform_checks(&self) -> anyhow::Result<()>;
}

impl ConfigChecks for StratusConfig {
    fn perform_checks(&self) -> anyhow::Result<()> {
        match self.mode {
            StratusMode::Leader =>
                if self.importer.is_some() {
                    return Err(anyhow::anyhow!("importer config must not be set when stratus mode is leader"));
                },
            StratusMode::Follower =>
                if self.importer.is_none() {
                    return Err(anyhow::anyhow!("importer config must be set when stratus mode is follower"));
                },
        }
        Ok(())
    }
}
