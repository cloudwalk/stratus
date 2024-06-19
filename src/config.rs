//! Application configuration.

use std::cmp::max;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use clap::Parser;
use display_json::DebugAsJson;
use strum::VariantNames;
use tokio::runtime::Builder;
use tokio::runtime::Runtime;

use crate::eth::evm::EvmConfig;
#[cfg(feature = "dev")]
use crate::eth::primitives::test_accounts;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
use crate::eth::relayer::ExternalRelayer;
use crate::eth::relayer::ExternalRelayerClient;
use crate::eth::storage::ExternalRpcStorage;
use crate::eth::storage::InMemoryPermanentStorage;
use crate::eth::storage::InMemoryTemporaryStorage;
use crate::eth::storage::PermanentStorage;
use crate::eth::storage::PostgresExternalRpcStorage;
use crate::eth::storage::PostgresExternalRpcStorageConfig;
#[cfg(feature = "rocks")]
use crate::eth::storage::RocksPermanentStorage;
use crate::eth::storage::StratusStorage;
use crate::eth::storage::TemporaryStorage;
use crate::eth::BlockMiner;
use crate::eth::BlockMinerMode;
use crate::eth::Executor;
use crate::eth::TransactionRelayer;
use crate::ext::parse_duration;
use crate::infra::build_info;
use crate::infra::tracing::TracingLogFormat;
use crate::infra::tracing::TracingProtocol;
use crate::infra::BlockchainClient;

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
}

/// Configuration that can be used by any binary.
#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
#[command(author, version, about, long_about = None)]
pub struct CommonConfig {
    /// Environment where the application is running.
    #[arg(long = "env", env = "ENV", default_value = "local")]
    pub env: Environment,

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
    #[arg(long = "tokio-console-address", env = "TRACING_TOKIO_CONSOLE_ADDRESS", default_value = "0.0.0.0:6669")]
    pub tokio_console_address: SocketAddr,

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

#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
pub struct TracingConfig {
    #[arg(long = "tracing-log-format", env = "TRACING_LOG_FORMAT", default_value = "normal")]
    pub tracing_log_format: TracingLogFormat,

    #[arg(long = "tracing-url", alias = "tracing-collector-url", env = "TRACING_URL")]
    pub tracing_url: Option<String>,

    #[arg(long = "tracing-headers", env = "TRACING_HEADERS", value_delimiter = ',')]
    pub tracing_headers: Vec<String>,

    #[arg(long = "tracing-protocol", env = "TRACING_PROTOCOL", default_value = "grpc")]
    pub tracing_protocol: TracingProtocol,
}

// -----------------------------------------------------------------------------
// Config: StratusStorage
// -----------------------------------------------------------------------------

/// Configuration that can be used by any binary that interacts with Stratus storage.
#[derive(Parser, DebugAsJson, Clone, serde::Serialize)]
pub struct StratusStorageConfig {
    #[clap(flatten)]
    pub temp_storage: TemporaryStorageConfig,

    #[clap(flatten)]
    pub perm_storage: PermanentStorageConfig,
}

impl StratusStorageConfig {
    /// Initializes Stratus storage.
    pub async fn init(&self) -> anyhow::Result<Arc<StratusStorage>> {
        let temp_storage = self.temp_storage.init().await?;
        let perm_storage = self.perm_storage.init().await?;
        let storage = StratusStorage::new(temp_storage, perm_storage);

        Ok(Arc::new(storage))
    }
}

// -----------------------------------------------------------------------------
// Config: Executor
// -----------------------------------------------------------------------------

#[derive(Parser, DebugAsJson, Clone, serde::Serialize)]
pub struct ExecutorConfig {
    /// Chain ID of the network.
    #[arg(long = "chain-id", env = "CHAIN_ID")]
    pub chain_id: u64,

    /// Number of EVM instances to run.
    #[arg(long = "evms", env = "EVMS")]
    pub num_evms: usize,
}

impl ExecutorConfig {
    /// Initializes Executor.
    ///
    /// Note: Should be called only after async runtime is initialized.
    pub async fn init(&self, storage: Arc<StratusStorage>, miner: Arc<BlockMiner>) -> Arc<Executor> {
        let config = EvmConfig {
            num_evms: max(self.num_evms, 1),
            chain_id: self.chain_id.into(),
        };
        tracing::info!(?config, "creating executor");

        let executor = Executor::new(storage, miner, config);
        Arc::new(executor)
    }
}

// -----------------------------------------------------------------------------
// Config: Miner
// -----------------------------------------------------------------------------
#[derive(Parser, DebugAsJson, Clone, serde::Serialize)]
pub struct MinerConfig {
    /// Target block time.
    #[arg(long = "block-mode", env = "BLOCK_MODE", default_value = "automine")]
    pub block_mode: BlockMinerMode,

    /// Generates genesis block on startup when it does not exist.
    #[arg(long = "enable-genesis", env = "ENABLE_GENESIS", default_value = "false")]
    pub enable_genesis: bool,

    /// Enables test accounts with max wei on startup.
    #[cfg(feature = "dev")]
    #[arg(long = "enable-test-accounts", env = "ENABLE_TEST_ACCOUNTS", default_value = "false")]
    pub enable_test_accounts: bool,
}

impl MinerConfig {
    /// Inits [`BlockMiner`] with external mining mode, ignoring the configured value.
    pub async fn init_external_mode(&self, storage: Arc<StratusStorage>, relayer: Option<ExternalRelayerClient>) -> anyhow::Result<Arc<BlockMiner>> {
        self.init_with_mode(BlockMinerMode::External, storage, relayer).await
    }

    /// Inits [`BlockMiner`] with the configured mining mode.
    pub async fn init(&self, storage: Arc<StratusStorage>, relayer: Option<ExternalRelayerClient>) -> anyhow::Result<Arc<BlockMiner>> {
        self.init_with_mode(self.block_mode, storage, relayer).await
    }

    async fn init_with_mode(
        &self,
        mode: BlockMinerMode,
        storage: Arc<StratusStorage>,
        relayer: Option<ExternalRelayerClient>,
    ) -> anyhow::Result<Arc<BlockMiner>> {
        tracing::info!(config = ?self, "creating block miner");

        // create miner
        let miner = BlockMiner::new(Arc::clone(&storage), mode, relayer);
        let miner = Arc::new(miner);

        // enable genesis block
        if self.enable_genesis {
            let genesis = storage.read_block(&BlockSelection::Number(BlockNumber::ZERO))?;
            if genesis.is_none() {
                tracing::info!("enabling genesis block");
                miner.commit(Block::genesis())?;
            }
        }

        // enable test accounts
        #[cfg(feature = "dev")]
        if self.enable_test_accounts {
            let test_accounts = test_accounts();
            tracing::info!(accounts = ?test_accounts, "enabling test accounts");
            storage.save_accounts(test_accounts)?;
        }

        // set block number
        storage.set_active_block_number_as_next_if_not_set()?;

        // enable interval miner
        if miner.mode().is_interval() {
            Arc::clone(&miner).spawn_interval_miner()?;
        }

        Ok(miner)
    }
}

// -----------------------------------------------------------------------------
// Config: Relayer
// -----------------------------------------------------------------------------
#[derive(Parser, DebugAsJson, Clone, serde::Serialize)]
pub struct IntegratedRelayerConfig {
    /// RPC address to forward transactions to.
    #[arg(long = "forward-to", env = "FORWARD_TO")]
    pub forward_to: Option<String>,

    /// Timeout for blockchain requests (relayer)
    #[arg(long = "relayer-timeout", value_parser=parse_duration, env = "RELAYER_TIMEOUT", default_value = "2s")]
    pub relayer_timeout: Duration,
}

impl IntegratedRelayerConfig {
    pub async fn init(&self) -> anyhow::Result<Option<Arc<TransactionRelayer>>> {
        tracing::info!(config = ?self, "creating transaction relayer");

        match self.forward_to {
            Some(ref forward_to) => {
                let chain = BlockchainClient::new_http(forward_to, self.relayer_timeout).await?;
                let relayer = TransactionRelayer::new(Arc::new(chain));
                Ok(Some(Arc::new(relayer)))
            }
            None => Ok(None),
        }
    }
}

#[derive(Parser, DebugAsJson, Clone, serde::Serialize)]
#[group(requires_all = ["url", "connections", "acquire_timeout"])]
pub struct ExternalRelayerClientConfig {
    #[arg(long = "relayer-db-url", env = "RELAYER_DB_URL", required = false)]
    pub url: String,
    #[arg(long = "relayer-db-connections", env = "RELAYER_DB_CONNECTIONS", required = false)]
    pub connections: u32,
    #[arg(long = "relayer-db-timeout", value_parser=parse_duration, env = "RELAYER_DB_TIMEOUT", required = false)]
    pub acquire_timeout: Duration,
}

impl ExternalRelayerClientConfig {
    pub async fn init(self) -> ExternalRelayerClient {
        ExternalRelayerClient::new(self).await
    }
}

#[derive(Parser, DebugAsJson, Clone, serde::Serialize)]
pub struct ExternalRelayerServerConfig {
    /// Postgresql url.
    #[arg(long = "db-url", env = "DB_URL")]
    pub url: String,

    /// Connections to database.
    #[arg(long = "db-connections", env = "DB_CONNECTIONS", default_value = "5")]
    pub connections: u32,

    /// Timeout to acquire connections to the database.
    #[arg(long = "db-timeout", value_parser=parse_duration, env = "DB_TIMEOUT", default_value = "1s")]
    pub acquire_timeout: Duration,

    /// RPC to forward to.
    #[arg(long = "forward-to", env = "RELAYER_FORWARD_TO")]
    pub forward_to: String,

    /// Backoff.
    #[arg(long = "backoff", value_parser=parse_duration, env = "BACKOFF", default_value = "10ms")]
    pub backoff: Duration,

    /// RPC response timeout.
    #[arg(long = "rpc-timeout", value_parser=parse_duration, env = "RPC_TIMEOUT", default_value = "2s")]
    pub rpc_timeout: Duration,
}

impl ExternalRelayerServerConfig {
    pub async fn init(self) -> anyhow::Result<ExternalRelayer> {
        ExternalRelayer::new(self).await
    }
}

// -----------------------------------------------------------------------------
// Config: Stratus
// -----------------------------------------------------------------------------

/// Configuration for main Stratus service.
#[derive(DebugAsJson, Clone, Parser, derive_more::Deref, serde::Serialize)]
pub struct StratusConfig {
    /// JSON-RPC binding address.
    #[arg(short = 'a', long = "address", env = "ADDRESS", default_value = "0.0.0.0:3000")]
    pub address: SocketAddr,

    /// JSON-RPC max active connections
    #[arg(long = "max_connections", env = "MAX_CONNECTIONS", default_value = "200")]
    pub max_connections: u32,

    #[clap(flatten)]
    pub storage: StratusStorageConfig,

    #[clap(flatten)]
    pub executor: ExecutorConfig,

    #[clap(flatten)]
    pub relayer: IntegratedRelayerConfig,

    #[clap(flatten)]
    pub external_relayer: Option<ExternalRelayerClientConfig>,

    #[clap(flatten)]
    pub miner: MinerConfig,

    #[deref]
    #[clap(flatten)]
    pub common: CommonConfig,
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
    pub relayer: IntegratedRelayerConfig,

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
}

#[derive(DebugAsJson, Clone, Parser, derive_more::Deref, serde::Serialize)]
pub struct RunWithImporterConfig {
    /// JSON-RPC binding address.
    #[arg(short = 'a', long = "address", env = "ADDRESS", default_value = "0.0.0.0:3000")]
    pub address: SocketAddr,

    /// JSON-RPC max active connections
    #[arg(long = "max_connections", env = "MAX_CONNECTIONS", default_value = "200")]
    pub max_connections: u32,

    #[arg(long = "leader_node", env = "LEADER_NODE")]
    pub leader_node: Option<String>, // to simulate this in use locally with other nodes, you need to add the node name into /etc/hostname

    #[clap(flatten)]
    pub online: ImporterOnlineBaseConfig,

    #[clap(flatten)]
    pub storage: StratusStorageConfig,

    #[clap(flatten)]
    pub executor: ExecutorConfig,

    #[clap(flatten)]
    pub relayer: IntegratedRelayerConfig,

    #[clap(flatten)]
    pub external_relayer: Option<ExternalRelayerClientConfig>,

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
    pub relayer: IntegratedRelayerConfig,

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
}

// -----------------------------------------------------------------------------
// Config: ExternalRpcStorage
// -----------------------------------------------------------------------------

/// External RPC storage configuration.
#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
pub struct ExternalRpcStorageConfig {
    /// External RPC storage implementation.
    #[arg(long = "external-rpc-storage", env = "EXTERNAL_RPC_STORAGE")]
    pub external_rpc_storage_kind: ExternalRpcStorageKind,

    /// External RPC storage number of parallel open connections.
    #[arg(long = "external-rpc-storage-connections", env = "EXTERNAL_RPC_STORAGE_CONNECTIONS")]
    pub external_rpc_storage_connections: u32,

    /// External RPC storage timeout when opening a connection.
    #[arg(long = "external-rpc-storage-timeout", value_parser=parse_duration, env = "EXTERNAL_RPC_STORAGE_TIMEOUT")]
    pub external_rpc_storage_timeout: Duration,
}

#[derive(DebugAsJson, Clone, serde::Serialize)]
pub enum ExternalRpcStorageKind {
    Postgres { url: String },
}

impl ExternalRpcStorageConfig {
    /// Initializes external rpc storage implementation.
    pub async fn init(&self) -> anyhow::Result<Arc<dyn ExternalRpcStorage>> {
        tracing::info!(config = ?self, "creating external rpc storage");

        match self.external_rpc_storage_kind {
            ExternalRpcStorageKind::Postgres { ref url } => {
                let config = PostgresExternalRpcStorageConfig {
                    url: url.to_owned(),
                    connections: self.external_rpc_storage_connections,
                    acquire_timeout: self.external_rpc_storage_timeout,
                };
                Ok(Arc::new(PostgresExternalRpcStorage::new(config).await?))
            }
        }
    }
}

impl FromStr for ExternalRpcStorageKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self, Self::Err> {
        match s {
            s if s.starts_with("postgres://") => Ok(Self::Postgres { url: s.to_string() }),
            s => Err(anyhow!("unknown external rpc storage: {}", s)),
        }
    }
}

// -----------------------------------------------------------------------------
// Config: ExternalRelayer
// -----------------------------------------------------------------------------

#[derive(DebugAsJson, Clone, Parser, derive_more::Deref, serde::Serialize)]
pub struct ExternalRelayerConfig {
    #[clap(flatten)]
    pub relayer: ExternalRelayerServerConfig,

    #[deref]
    #[clap(flatten)]
    pub common: CommonConfig,
}

impl WithCommonConfig for ExternalRelayerConfig {
    fn common(&self) -> &CommonConfig {
        &self.common
    }
}

// -----------------------------------------------------------------------------
// Enum: Env
// -----------------------------------------------------------------------------
#[derive(DebugAsJson, strum::Display, strum::VariantNames, Clone, Copy, Parser, serde::Serialize)]
pub enum Environment {
    #[strum(to_string = "local")]
    Local,

    #[strum(to_string = "staging")]
    Staging,

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
// Enum: TemporaryStorageConfig
// -----------------------------------------------------------------------------

/// Temporary storage configuration.
#[derive(Parser, DebugAsJson, Clone, serde::Serialize)]
pub struct TemporaryStorageConfig {
    /// Temporary storage implementation.
    #[arg(long = "temp-storage", env = "TEMP_STORAGE")]
    pub temp_storage_kind: TemporaryStorageKind,
}

#[derive(DebugAsJson, Clone, serde::Serialize)]
pub enum TemporaryStorageKind {
    InMemory,
}

impl TemporaryStorageConfig {
    /// Initializes temporary storage implementation.
    pub async fn init(&self) -> anyhow::Result<Arc<dyn TemporaryStorage>> {
        tracing::info!(config = ?self, "creating temporary storage");

        match self.temp_storage_kind {
            TemporaryStorageKind::InMemory => Ok(Arc::new(InMemoryTemporaryStorage::default())),
        }
    }
}

impl FromStr for TemporaryStorageKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self, Self::Err> {
        match s {
            "inmemory" => Ok(Self::InMemory),
            s => Err(anyhow!("unknown temporary storage: {}", s)),
        }
    }
}

// -----------------------------------------------------------------------------
// Enum: PermanentStorageConfig
// -----------------------------------------------------------------------------

/// Permanent storage configuration.
#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
pub struct PermanentStorageConfig {
    /// Permamenent storage implementation.
    #[arg(long = "perm-storage", env = "PERM_STORAGE")]
    pub perm_storage_kind: PermanentStorageKind,

    #[cfg(feature = "rocks")]
    /// RocksDB storage path prefix to execute multiple local Stratus instances.
    #[arg(long = "rocks-path-prefix", env = "ROCKS_PATH_PREFIX", default_value = "")]
    pub rocks_path_prefix: Option<String>,
}

#[derive(DebugAsJson, Clone, serde::Serialize)]
pub enum PermanentStorageKind {
    InMemory,
    #[cfg(feature = "rocks")]
    Rocks,
}

impl PermanentStorageConfig {
    /// Initializes permanent storage implementation.
    pub async fn init(&self) -> anyhow::Result<Arc<dyn PermanentStorage>> {
        tracing::info!(config = ?self, "creating permanent storage");

        let perm: Arc<dyn PermanentStorage> = match self.perm_storage_kind {
            PermanentStorageKind::InMemory => Arc::new(InMemoryPermanentStorage::default()),
            #[cfg(feature = "rocks")]
            PermanentStorageKind::Rocks => Arc::new(RocksPermanentStorage::new(self.rocks_path_prefix.clone())?),
        };
        Ok(perm)
    }
}

impl FromStr for PermanentStorageKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self, Self::Err> {
        match s {
            "inmemory" => Ok(Self::InMemory),
            #[cfg(feature = "rocks")]
            "rocks" => Ok(Self::Rocks),
            s => Err(anyhow!("unknown permanent storage: {}", s)),
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
