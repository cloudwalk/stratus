//! Application configuration.

use std::cmp::max;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::anyhow;
use clap::Parser;
use display_json::DebugAsJson;
use tokio::runtime::Builder;
use tokio::runtime::Handle;
use tokio::runtime::Runtime;

use crate::bin_name;
use crate::eth::evm::revm::Revm;
use crate::eth::evm::Evm;
use crate::eth::evm::EvmConfig;
#[cfg(feature = "dev")]
use crate::eth::primitives::test_accounts;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
#[cfg(feature = "dev")]
use crate::eth::primitives::StoragePointInTime;
use crate::eth::storage::ExternalRpcStorage;
use crate::eth::storage::InMemoryPermanentStorage;
use crate::eth::storage::InMemoryTemporaryStorage;
use crate::eth::storage::PermanentStorage;
use crate::eth::storage::PostgresExternalRpcStorage;
use crate::eth::storage::PostgresExternalRpcStorageConfig;
use crate::eth::storage::PostgresPermanentStorage;
use crate::eth::storage::PostgresPermanentStorageConfig;
#[cfg(feature = "rocks")]
use crate::eth::storage::RocksPermanentStorage;
use crate::eth::storage::StratusStorage;
use crate::eth::storage::TemporaryStorage;
use crate::eth::BlockMiner;
use crate::eth::EvmTask;
use crate::eth::Executor;
use crate::eth::TransactionRelayer;
#[cfg(feature = "dev")]
use crate::ext::not;
use crate::infra::BlockchainClient;

/// Loads .env files according to the binary and environment.
pub fn load_dotenv() {
    let env = std::env::var("ENV").unwrap_or_else(|_| "local".to_string());
    let env_filename = format!("config/{}.env.{}", bin_name(), env);

    println!("reading env file: {}", env_filename);
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
    /// Number of threads to execute global async tasks.
    #[arg(long = "async-threads", env = "ASYNC_THREADS", default_value = "10")]
    pub num_async_threads: usize,

    /// Number of threads to execute global blocking tasks.
    #[arg(long = "blocking-threads", env = "BLOCKING_THREADS", default_value = "10")]
    pub num_blocking_threads: usize,

    #[arg(long = "metrics-histogram-kind", env = "METRICS_HISTOGRAM_KIND", default_value = "summary")]
    pub metrics_histogram_kind: MetricsHistogramKind,

    /// Prevents clap from breaking when passing `nocapture` options in tests.
    #[arg(long = "nocapture")]
    pub nocapture: bool,

    /// Url to the sentry project
    #[arg(long = "sentry-url", env = "SENTRY_URL")]
    pub sentry_url: Option<String>,

    /// Url to the tracing collector (Opentelemetry over gRPC)
    #[arg(long = "tracing-collector-url", env = "TRACING_COLLECTOR_URL")]
    pub tracing_url: Option<String>,
}

impl WithCommonConfig for CommonConfig {
    fn common(&self) -> &CommonConfig {
        self
    }
}

impl CommonConfig {
    /// Initializes Tokio runtime.
    pub fn init_runtime(&self) -> Runtime {
        print!(
            "starting tokio runtime; async_threads={}; blocking_threads={}",
            self.num_async_threads, self.num_blocking_threads
        );

        let runtime = Builder::new_multi_thread()
            .enable_all()
            .thread_name("tokio")
            .worker_threads(self.num_async_threads)
            .max_blocking_threads(self.num_blocking_threads)
            .thread_keep_alive(Duration::from_secs(u64::MAX))
            .build()
            .expect("failed to start tokio runtime");

        runtime
    }
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

    /// Generates genesis block on startup when it does not exist.
    #[arg(long = "enable-genesis", env = "ENABLE_GENESIS", default_value = "false")]
    pub enable_genesis: bool,

    /// Enables test accounts with max wei on startup.
    #[cfg(feature = "dev")]
    #[arg(long = "enable-test-accounts", env = "ENABLE_TEST_ACCOUNTS", default_value = "false")]
    pub enable_test_accounts: bool,
}

impl StratusStorageConfig {
    /// Initializes Stratus storage.
    pub async fn init(&self) -> anyhow::Result<Arc<StratusStorage>> {
        let temp_storage = self.temp_storage.init().await?;
        let perm_storage = self.perm_storage.init().await?;

        let storage = StratusStorage::new(temp_storage, perm_storage);
        storage.set_active_block_number_as_next_if_not_set().await?;

        // enable genesis block
        if self.enable_genesis {
            let genesis = storage.read_block(&BlockSelection::Number(BlockNumber::ZERO)).await?;
            if genesis.is_none() {
                tracing::info!("enabling genesis block");
                storage.save_block(Block::genesis()).await?;
            }
        }

        // enable test accounts
        #[cfg(feature = "dev")]
        if self.enable_test_accounts {
            let mut test_accounts_to_insert = Vec::new();
            for test_account in test_accounts() {
                let storage_account = storage.read_account(&test_account.address, &StoragePointInTime::Present).await?;
                if storage_account.is_empty() {
                    test_accounts_to_insert.push(test_account);
                }
            }

            if not(test_accounts_to_insert.is_empty()) {
                tracing::info!(accounts = ?test_accounts_to_insert, "enabling test accounts");
                storage.save_accounts(test_accounts_to_insert).await?;
            }
        }

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
    ///
    /// TODO: remove BlockMiner after migration is completed.
    pub async fn init(&self, storage: Arc<StratusStorage>, miner: Arc<BlockMiner>, relayer: Option<Arc<TransactionRelayer>>) -> Arc<Executor> {
        let num_evms = max(self.num_evms, 1);
        tracing::info!(config = ?self, "starting executor");

        // spawn evm in background using native threads
        let (evm_tx, evm_rx) = crossbeam_channel::unbounded::<EvmTask>();
        for _ in 1..=num_evms {
            // create evm resources
            let evm_config = EvmConfig {
                chain_id: self.chain_id.into(),
                prefetch_slots: matches!(storage.perm_kind(), PermanentStorageKind::Postgres { .. }),
            };
            let evm_storage = Arc::clone(&storage);
            let evm_tokio = Handle::current();
            let evm_rx = evm_rx.clone();

            // spawn thread that will run evm
            // todo: needs a way to signal error like a cancellation token in case it fails to initialize
            let t = thread::Builder::new().name("evm".into());
            t.spawn(move || {
                // init tokio
                let _tokio_guard = evm_tokio.enter();

                // init storage
                if let Err(e) = Handle::current().block_on(evm_storage.allocate_evm_thread_resources()) {
                    tracing::error!(reason = ?e, "failed to allocate evm storage resources");
                }

                // init evm
                let mut evm = Revm::new(evm_storage, evm_config);

                // keep executing transactions until the channel is closed
                while let Ok((input, tx)) = evm_rx.recv() {
                    let result = evm.execute(input);
                    if let Err(e) = tx.send(result) {
                        tracing::error!(reason = ?e, "failed to send evm execution result");
                    };
                }
                tracing::warn!("stopping evm thread because task channel was closed");
            })
            .expect("spawning evm threads should not fail");
        }

        let auto_mine_enabled = if miner.block_time.is_none() {
            tracing::info!("enabling auto mining");
            true
        } else {
            false
        };

        let executor = Executor::new(storage, miner, auto_mine_enabled, relayer, evm_tx, self.num_evms);
        Arc::new(executor)
    }
}

// -----------------------------------------------------------------------------
// Config: Miner
// -----------------------------------------------------------------------------
#[derive(Parser, DebugAsJson, Clone, serde::Serialize)]
pub struct MinerConfig {
    /// Target block time.
    #[arg(long = "block-time", value_parser=parse_duration, env = "BLOCK_TIME")]
    pub block_time: Option<Duration>,
}

impl MinerConfig {
    pub fn init(&self, storage: Arc<StratusStorage>) -> Arc<BlockMiner> {
        tracing::info!(config = ?self, "starting block miner");
        let miner = Arc::new(BlockMiner::new(storage, self.block_time));

        if let Some(block_time) = self.block_time {
            Arc::clone(&miner).spawn_interval_miner(block_time);
        }

        miner
    }
}

// -----------------------------------------------------------------------------
// Config: Relayer
// -----------------------------------------------------------------------------
#[derive(Parser, DebugAsJson, Clone, serde::Serialize)]
pub struct RelayerConfig {
    /// RPC address to forward transactions to.
    #[arg(long = "forward-to", env = "FORWARD_TO")]
    pub forward_to: Option<String>,
}

impl RelayerConfig {
    pub async fn init(&self, storage: Arc<StratusStorage>) -> anyhow::Result<Option<Arc<TransactionRelayer>>> {
        tracing::info!(config = ?self, "starting transaction relayer");

        match self.forward_to {
            Some(ref url) => {
                let chain = BlockchainClient::new(url).await?;
                let relayer = TransactionRelayer::new(storage, chain);
                Ok(Some(Arc::new(relayer)))
            }
            None => Ok(None),
        }
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

    #[clap(flatten)]
    pub storage: StratusStorageConfig,

    #[clap(flatten)]
    pub executor: ExecutorConfig,

    #[clap(flatten)]
    pub relayer: RelayerConfig,

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
    #[clap(flatten)]
    pub rpc_storage: ExternalRpcStorageConfig,

    /// External RPC endpoint to sync blocks with Stratus.
    #[arg(short = 'r', long = "external-rpc", env = "EXTERNAL_RPC")]
    pub external_rpc: String,

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

    /// Write data to CSV file instead of permanent storage.
    #[arg(long = "export-csv", env = "EXPORT_CSV", default_value = "false")]
    pub export_csv: bool,

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
    /// External RPC endpoint to sync blocks with Stratus.
    #[arg(short = 'r', long = "external-rpc", env = "EXTERNAL_RPC")]
    pub external_rpc: String,

    #[arg(long = "sync-interval", value_parser=parse_duration, env = "SYNC_INTERVAL", default_value = "600ms")]
    pub sync_interval: Duration,

    #[clap(flatten)]
    pub executor: ExecutorConfig,

    #[clap(flatten)]
    pub relayer: RelayerConfig,

    #[clap(flatten)]
    pub miner: MinerConfig,

    #[clap(flatten)]
    pub storage: StratusStorageConfig,

    #[deref]
    #[clap(flatten)]
    pub common: CommonConfig,
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

    #[arg(long = "leader_node", env = "LEADER_NODE")]
    pub leader_node: Option<String>, // to simulate this in use locally with other nodes, you need to add the node name into /etc/hostname

    #[arg(long = "sync-interval", value_parser=parse_duration, env = "SYNC_INTERVAL", default_value = "600ms")]
    pub sync_interval: Duration,

    #[clap(flatten)]
    pub storage: StratusStorageConfig,

    #[clap(flatten)]
    pub executor: ExecutorConfig,

    #[clap(flatten)]
    pub relayer: RelayerConfig,

    #[clap(flatten)]
    pub miner: MinerConfig,

    /// External RPC endpoint to sync blocks with Stratus.
    #[arg(short = 'r', long = "external-rpc", env = "EXTERNAL_RPC")]
    pub external_rpc: String,

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
    pub relayer: RelayerConfig,

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
        tracing::info!(config = ?self, "starting external rpc storage");

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
        tracing::info!(config = ?self, "starting temporary storage");

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

    /// Permamenent storage number of parallel open connections.
    #[arg(long = "perm-storage-connections", env = "PERM_STORAGE_CONNECTIONS")]
    pub perm_storage_connections: u32,

    /// Permamenent storage timeout when opening a connection (in millis).
    #[arg(long = "perm-storage-timeout", value_parser=parse_duration, env = "PERM_STORAGE_TIMEOUT")]
    pub perm_storage_timeout: Duration,
}

#[derive(DebugAsJson, Clone, serde::Serialize)]
pub enum PermanentStorageKind {
    InMemory,
    #[cfg(feature = "rocks")]
    Rocks,
    Postgres {
        url: String,
    },
}

impl PermanentStorageConfig {
    /// Initializes permanent storage implementation.
    pub async fn init(&self) -> anyhow::Result<Arc<dyn PermanentStorage>> {
        tracing::info!(config = ?self, "starting permanent storage");

        let perm: Arc<dyn PermanentStorage> = match self.perm_storage_kind {
            PermanentStorageKind::InMemory => Arc::new(InMemoryPermanentStorage::default()),
            #[cfg(feature = "rocks")]
            PermanentStorageKind::Rocks => Arc::new(RocksPermanentStorage::new().await?),
            PermanentStorageKind::Postgres { ref url } => {
                let config = PostgresPermanentStorageConfig {
                    url: url.to_owned(),
                    connections: self.perm_storage_connections,
                    acquire_timeout: self.perm_storage_timeout,
                };
                Arc::new(PostgresPermanentStorage::new(config).await?)
            }
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
            s if s.starts_with("postgres://") => Ok(Self::Postgres { url: s.to_string() }),
            s => Err(anyhow!("unknown permanent storage: {}", s)),
        }
    }
}

// -----------------------------------------------------------------------------
// Enum: MetricsHistogramKind
// -----------------------------------------------------------------------------

/// See: <https://prometheus.io/docs/practices/histograms/>
#[derive(DebugAsJson, Clone, Copy, Eq, PartialEq, serde::Serialize)]
pub enum MetricsHistogramKind {
    /// Quantiles are calculated on client-side based on recent data kept in-memory.
    ///
    /// Client defines the quantiles to calculate.
    Summary,

    /// Quantiles are calculated on server-side based on bucket counts.
    ///
    /// Cient defines buckets to group observations.
    Histogram,
}

impl FromStr for MetricsHistogramKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self, Self::Err> {
        match s.to_lowercase().trim() {
            "summary" => Ok(Self::Summary),
            "histogram" => Ok(Self::Histogram),
            s => Err(anyhow!("unknown metrics histogram kind: {}", s)),
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
// Parsers
// -----------------------------------------------------------------------------
fn parse_duration(s: &str) -> anyhow::Result<Duration> {
    // try millis
    let millis: Result<u64, _> = s.parse();
    if let Ok(millis) = millis {
        return Ok(Duration::from_millis(millis));
    }

    // try humantime
    if let Ok(parsed) = humantime::parse_duration(s) {
        return Ok(parsed);
    }

    // error
    Err(anyhow!("invalid duration format: {}", s))
}
