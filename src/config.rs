//! Application configuration.

use std::cmp::max;
use std::fmt::Debug;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use clap::Parser;
use nonempty::NonEmpty;
use tokio::runtime::Builder;
use tokio::runtime::Runtime;

use crate::eth::evm::revm::Revm;
use crate::eth::evm::Evm;
use crate::eth::miner::BlockMiner;
use crate::eth::storage::test_accounts;
use crate::eth::storage::EthStorage;
use crate::eth::storage::InMemoryStorage;
use crate::eth::EthExecutor;
use crate::ext::not;
use crate::infra::postgres::Postgres;

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

/// Configuration for importer-download binary.
#[derive(Parser, Debug)]
pub struct ImporterDownloadConfig {
    /// External RPC endpoint to sync blocks with Stratus.
    #[arg(short = 'r', long = "external-rpc", env = "EXTERNAL_RPC")]
    pub external_rpc: String,

    /// Postgres connection URL.
    #[arg(long = "postgres", env = "POSTGRES_URL")]
    pub postgres_url: String,
}

/// Configuration for importer-import binary.
#[derive(Parser, Debug, derive_more::Deref)]
pub struct ImporterImportConfig {
    /// Postgres connection URL.
    #[arg(short = 'd', long = "postgres", env = "POSTGRES_URL")]
    pub postgres_url: String,

    #[deref]
    #[clap(flatten)]
    pub common: CommonConfig,
}

/// Configuration for rpc-poller binary.
#[derive(Parser, Debug, derive_more::Deref)]
pub struct RpcPollerConfig {
    /// External RPC endpoint to sync blocks with Stratus.
    #[arg(short = 'r', long = "external-rpc", env = "EXTERNAL_RPC")]
    pub external_rpc: String,

    #[deref]
    #[clap(flatten)]
    pub common: CommonConfig,
}

/// Common configuration that can be used by any binary.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct CommonConfig {
    /// Environment where the application is running.
    #[arg(value_enum, short = 'e', long = "env", env = "ENV", default_value_t = Environment::Development)]
    pub env: Environment,

    /// Storage implementation.
    #[arg(short = 's', long = "storage", env = "STORAGE", default_value_t = StorageConfig::InMemory)]
    pub storage: StorageConfig,

    /// Number of EVM instances to run.
    #[arg(long = "evms", env = "EVMS", default_value = "1")]
    pub num_evms: usize,

    /// Number of threads to execute global async tasks.
    #[arg(long = "async-threads", env = "ASYNC_THREADS", default_value = "1")]
    pub num_async_threads: usize,

    /// Number of threads to execute global blocking tasks.
    #[arg(long = "blocking-threads", env = "BLOCKING_THREADS", default_value = "1")]
    pub num_blocking_threads: usize,

    /// Generates genesis block on startup when it does not exist.
    #[arg(long = "enable-genesis", env = "ENABLE_GENESIS", default_value = "false")]
    pub enable_genesis: bool,

    /// Enables test accounts with max wei on startup.
    #[arg(long = "enable-test-accounts", env = "ENABLE_TEST_ACCOUNTS", default_value = "false")]
    pub enable_test_accounts: bool,
}

impl CommonConfig {
    /// Initializes storage.
    pub async fn init_storage(&self) -> anyhow::Result<Arc<dyn EthStorage>> {
        let storage = self.storage.init().await?;
        if self.enable_genesis {
            storage.enable_genesis(BlockMiner::genesis()).await?;
        }
        if self.enable_test_accounts {
            storage.enable_test_accounts(test_accounts()).await?;
        }
        Ok(storage)
    }

    /// Initializes EthExecutor.
    pub fn init_executor(&self, storage: Arc<dyn EthStorage>) -> EthExecutor {
        let num_evms = max(self.num_evms, 1);
        tracing::info!(evms = %num_evms, "starting executor");

        let mut evms: Vec<Box<dyn Evm>> = Vec::with_capacity(num_evms);
        for _ in 1..=num_evms {
            evms.push(Box::new(Revm::new(Arc::clone(&storage))));
        }

        EthExecutor::new(NonEmpty::from_vec(evms).unwrap(), Arc::clone(&storage))
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

/// Storage configuration.
#[derive(Clone, Debug, strum::Display)]
pub enum StorageConfig {
    #[strum(serialize = "inmemory")]
    InMemory,

    #[strum(serialize = "postgres")]
    Postgres { url: String },
}

impl StorageConfig {
    /// Initializes the storage implementation.
    pub async fn init(&self) -> anyhow::Result<Arc<dyn EthStorage>> {
        match self {
            Self::InMemory => Ok(Arc::new(InMemoryStorage::default().metrified())),
            Self::Postgres { url } => Ok(Arc::new(Postgres::new(url).await?.metrified())),
        }
    }
}

impl FromStr for StorageConfig {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self, Self::Err> {
        match s {
            "inmemory" => Ok(Self::InMemory),
            s if s.starts_with("postgres://") => Ok(Self::Postgres { url: s.to_string() }),
            s => Err(anyhow!("unknown storage: {}", s)),
        }
    }
}

/// Enviroment where the application is running.
#[derive(clap::ValueEnum, Debug, Clone, PartialEq)]
pub enum Environment {
    Development,
    Staging,
    Production,
}

impl Environment {
    /// Checks if the current environment is production.
    pub fn is_production(&self) -> bool {
        matches!(self, Self::Production)
    }

    /// Checks if the current environment is NOT production.
    pub fn is_not_production(&self) -> bool {
        not(self.is_production())
    }
}

impl FromStr for Environment {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim().to_lowercase();
        match s.as_str() {
            "dev" | "development" => Ok(Self::Development),
            "stag" | "staging" => Ok(Self::Staging),
            "prod" | "production" => Ok(Self::Production),
            &_ => todo!(),
        }
    }
}
