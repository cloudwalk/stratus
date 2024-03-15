//! Application configuration.

use std::cmp::max;
use std::fmt::Debug;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::anyhow;
use clap::Parser;
use tokio::runtime::Builder;
use tokio::runtime::Handle;
use tokio::runtime::Runtime;

use crate::eth::evm::revm::Revm;
use crate::eth::evm::Evm;
#[cfg(feature = "dev")]
use crate::eth::primitives::test_accounts;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
#[cfg(feature = "dev")]
use crate::eth::primitives::StoragePointInTime;
use crate::eth::storage::ExternalRpcStorage;
use crate::eth::storage::HybridPermanentStorage;
use crate::eth::storage::InMemoryPermanentStorage;
use crate::eth::storage::InMemoryTemporaryStorage;
use crate::eth::storage::PermanentStorage;
use crate::eth::storage::PostgresExternalRpcStorage;
use crate::eth::storage::PostgresExternalRpcStorageConfig;
use crate::eth::storage::StratusStorage;
use crate::eth::storage::TemporaryStorage;
use crate::eth::BlockMiner;
use crate::eth::EthExecutor;
use crate::eth::EvmTask;
#[cfg(feature = "dev")]
use crate::ext::not;
use crate::infra::postgres::Postgres;
use crate::infra::postgres::PostgresClientConfig;

pub trait WithCommonConfig {
    fn common(&self) -> &CommonConfig;
}

// -----------------------------------------------------------------------------
// Config: Stratus
// -----------------------------------------------------------------------------

/// Configuration for main Stratus service.
#[derive(Parser, Debug, derive_more::Deref)]
pub struct StratusConfig {
    /// JSON-RPC binding address.
    #[arg(short = 'a', long = "address", env = "ADDRESS", default_value = "0.0.0.0:3000")]
    pub address: SocketAddr,

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
#[derive(Parser, Debug, derive_more::Deref)]
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
#[derive(Parser, Debug, derive_more::Deref)]
pub struct ImporterOfflineConfig {
    #[clap(flatten)]
    pub rpc_storage: ExternalRpcStorageConfig,

    /// Number of parallel database fetches.
    #[arg(short = 'p', long = "paralellism", env = "PARALELLISM", default_value = "1")]
    pub paralellism: usize,

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
#[derive(Parser, Debug, derive_more::Deref)]
pub struct ImporterOnlineConfig {
    /// External RPC endpoint to sync blocks with Stratus.
    #[arg(short = 'r', long = "external-rpc", env = "EXTERNAL_RPC")]
    pub external_rpc: String,

    #[deref]
    #[clap(flatten)]
    pub common: CommonConfig,
}

impl WithCommonConfig for ImporterOnlineConfig {
    fn common(&self) -> &CommonConfig {
        &self.common
    }
}

// -----------------------------------------------------------------------------
// Config: StateValidator
// -----------------------------------------------------------------------------

/// Configuration for `state-validator` binary.
#[derive(Parser, Debug, derive_more::Deref)]
pub struct StateValidatorConfig {
    #[deref]
    #[clap(flatten)]
    pub common: CommonConfig,

    /// How many slots to validate per batch. 0 means every slot.
    #[arg(long = "max-samples", env = "MAX_SAMPLES", default_value_t = 0)]
    pub sample_size: u64,

    /// Seed to use when sampling. 0 for random seed.
    #[arg(long = "seed", env = "SEED", default_value_t = 0, requires = "sample_size")]
    pub seed: u64,

    /// Validate in batches of n blocks.
    #[arg(short = 'i', long = "inverval", env = "INVERVAL", default_value_t = 1000)]
    pub interval: u64,

    /// What method to use when validating.
    #[arg(short = 'm', long = "method", env = "METHOD")]
    pub method: ValidatorMethodConfig,

    /// How many concurrent validation tasks to run
    #[arg(short = 'c', long = "concurrent-tasks", env = "CONCURRENT_TASKS", default_value_t = 10)]
    pub concurrent_tasks: u16,
}

impl WithCommonConfig for StateValidatorConfig {
    fn common(&self) -> &CommonConfig {
        &self.common
    }
}

// -----------------------------------------------------------------------------
// Config: Common
// -----------------------------------------------------------------------------

/// Common configuration that can be used by any binary.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct CommonConfig {
    #[clap(flatten)]
    pub temp_storage: TemporaryStorageConfig,

    #[clap(flatten)]
    pub perm_storage: PermanentStorageConfig,

    /// Number of EVM instances to run.
    #[arg(long = "evms", env = "EVMS", default_value = "1")]
    pub num_evms: usize,

    /// Number of threads to execute global async tasks.
    #[arg(long = "async-threads", env = "ASYNC_THREADS", default_value = "1")]
    pub num_async_threads: usize,

    /// Number of threads to execute global blocking tasks.
    #[arg(long = "blocking-threads", env = "BLOCKING_THREADS", default_value = "1")]
    pub num_blocking_threads: usize,

    #[arg(long = "metrics-histogram-kind", env = "METRICS_HISTOGRAM_KIND", default_value = "summary")]
    pub metrics_histogram_kind: MetricsHistogramKind,

    /// Generates genesis block on startup when it does not exist.
    #[arg(long = "enable-genesis", env = "ENABLE_GENESIS", default_value = "false")]
    pub enable_genesis: bool,

    /// Enables test accounts with max wei on startup.
    #[cfg(feature = "dev")]
    #[arg(long = "enable-test-accounts", env = "ENABLE_TEST_ACCOUNTS", default_value = "false")]
    pub enable_test_accounts: bool,

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
    /// Initializes Stratus storage.
    pub async fn init_stratus_storage(&self) -> anyhow::Result<Arc<StratusStorage>> {
        let temp_storage = self.temp_storage.init().await?;
        let perm_storage = self.perm_storage.init().await?;
        let storage = StratusStorage::new(temp_storage, perm_storage);

        if self.enable_genesis {
            let genesis = storage.read_block(&BlockSelection::Number(BlockNumber::ZERO)).await?;
            if genesis.is_none() {
                tracing::info!("enabling genesis block");
                storage.commit_to_perm(BlockMiner::genesis()).await?;
            }
        }

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
                storage.save_accounts_to_perm(test_accounts_to_insert).await?;
            }
        }

        Ok(Arc::new(storage))
    }

    /// Initializes EthExecutor. Should be called inside an async runtime.
    pub fn init_executor(&self, storage: Arc<StratusStorage>) -> EthExecutor {
        let num_evms = max(self.num_evms, 1);
        tracing::info!(evms = %num_evms, "starting executor and evms");

        // spawn evm in background using native threads
        let (evm_tx, evm_rx) = crossbeam_channel::unbounded::<EvmTask>();
        for _ in 1..=num_evms {
            // create evm resources
            let evm_storage = Arc::clone(&storage);
            let evm_tokio = Handle::current();
            let evm_rx = evm_rx.clone();

            // spawn thread that will run evm
            let t = thread::Builder::new().name("evm".into());
            t.spawn(move || {
                let _tokio_guard = evm_tokio.enter();
                let mut evm = Revm::new(evm_storage);

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

        // creates an executor that can communicate with background evms
        EthExecutor::new(evm_tx, Arc::clone(&storage))
    }

    /// Initializes Tokio runtime.
    pub fn init_runtime(&self) -> Runtime {
        tracing::info!(
            async_threads = %self.num_async_threads,
            blocking_threads = %self.num_blocking_threads,
            "starting tokio runtime"
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
// Enum: PostgresConfig
// -----------------------------------------------------------------------------

/// External RPC storage configuration.
#[derive(Parser, Debug)]
pub struct ExternalRpcStorageConfig {
    /// External RPC storage implementation.
    #[arg(long = "external-rpc-storage-kind", env = "EXTERNAL_RPC_STORAGE")]
    pub external_rpc_storage_kind: ExternalRpcStorageKind,

    /// External RPC storage number of parallel open connections.
    #[arg(long = "external-rpc-storage-connections", env = "EXTERNAL_RPC_STORAGE_CONNECTIONS", default_value = "5")]
    pub external_rpc_storage_connections: u32,

    /// External RPC storage timeout when opening a connection (in millis).
    #[arg(long = "external-rpc-storage-timeout", env = "EXTERNAL_RPC_STORAGE_TIMEOUT", default_value = "2000")]
    pub external_rpc_storage_timeout_millis: u64,
}

#[derive(Clone, Debug)]
pub enum ExternalRpcStorageKind {
    Postgres { url: String },
}

impl ExternalRpcStorageConfig {
    /// Initializes external rpc storage implementation.
    pub async fn init(&self) -> anyhow::Result<Arc<dyn ExternalRpcStorage>> {
        match self.external_rpc_storage_kind {
            ExternalRpcStorageKind::Postgres { ref url } => {
                let config = PostgresExternalRpcStorageConfig {
                    url: url.to_owned(),
                    connections: self.external_rpc_storage_connections,
                    acquire_timeout: Duration::from_millis(self.external_rpc_storage_timeout_millis),
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
#[derive(Parser, Debug)]
pub struct TemporaryStorageConfig {
    /// Temporary storage implementation.
    #[arg(long = "temp-storage", env = "TEMP_STORAGE", default_value = "inmemory")]
    pub temp_storage_kind: TemporaryStorageKind,
}

#[derive(Clone, Debug)]
pub enum TemporaryStorageKind {
    InMemory,
}

impl TemporaryStorageConfig {
    /// Initializes temporary storage implementation.
    pub async fn init(&self) -> anyhow::Result<Arc<dyn TemporaryStorage>> {
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
#[derive(Parser, Debug)]
pub struct PermanentStorageConfig {
    /// Permamenent storage implementation.
    #[arg(long = "perm-storage", env = "PERM_STORAGE", default_value = "inmemory")]
    pub perm_storage_kind: PermanentStorageKind,

    /// Permamenent storage number of parallel open connections.
    #[arg(long = "perm-storage-connections", env = "PERM_STORAGE_CONNECTIONS", default_value = "5")]
    pub perm_storage_connections: u32,

    /// Permamenent storage timeout when opening a connection (in millis).
    #[arg(long = "perm-storage-timeout", env = "PERM_STORAGE_TIMEOUT", default_value = "2000")]
    pub perm_storage_timeout_millis: u64,
}

#[derive(Clone, Debug)]
pub enum PermanentStorageKind {
    InMemory,
    Postgres { url: String },
    Hybrid { url: String },
}

impl PermanentStorageConfig {
    /// Initializes permanent storage implementation.
    pub async fn init(&self) -> anyhow::Result<Arc<dyn PermanentStorage>> {
        let perm: Arc<dyn PermanentStorage> = match self.perm_storage_kind {
            PermanentStorageKind::InMemory => Arc::new(InMemoryPermanentStorage::default()),

            PermanentStorageKind::Postgres { ref url } => {
                let config = PostgresClientConfig {
                    url: url.to_owned(),
                    connections: self.perm_storage_connections,
                    acquire_timeout: Duration::from_millis(self.perm_storage_timeout_millis),
                };
                Arc::new(Postgres::new(config).await?)
            }

            PermanentStorageKind::Hybrid { ref url } => {
                let config = PostgresClientConfig {
                    url: url.to_owned(),
                    connections: self.perm_storage_connections,
                    acquire_timeout: Duration::from_millis(self.perm_storage_timeout_millis),
                };
                Arc::new(HybridPermanentStorage::new(config).await?)
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
            s if s.starts_with("postgres://") => Ok(Self::Postgres { url: s.to_string() }),
            s if s.starts_with("hybrid://") => {
                let s = s.replace("hybrid", "postgres"); //TODO there is a better way to do this
                Ok(Self::Hybrid { url: s.to_string() })
            }
            s => Err(anyhow!("unknown permanent storage: {}", s)),
        }
    }
}

// -----------------------------------------------------------------------------
// Enum: MetricsHistogramKind
// -----------------------------------------------------------------------------

/// See: https://prometheus.io/docs/practices/histograms/
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

#[derive(Clone, Debug, strum::Display)]
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
