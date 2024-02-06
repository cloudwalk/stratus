//! Application configuration.

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
use crate::eth::storage::EthStorage;
use crate::eth::storage::InMemoryStorage;
use crate::ext::not;
use crate::infra::postgres::Postgres;

/// Application configuration entry-point.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Config {
    /// Environment where the application is running.
    #[arg(value_enum, short = 'e', long = "env", env = "ENV", default_value = "dev")]
    pub env: Environment,

    /// JSON-RPC binding address.
    #[arg(short = 'a', long = "address", env = "ADDRESS", default_value = "0.0.0.0:3000")]
    pub address: SocketAddr,

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

    /// External RPC endpoint to sync blocks with Stratus.
    #[arg(short = 'r', long = "external-rpc", env = "EXTERNAL_RPC")]
    pub external_rpc: Option<String>,
}

impl Config {
    /// Initializes storage.
    pub async fn init_storage(&self) -> anyhow::Result<Arc<dyn EthStorage>> {
        self.storage.init().await
    }

    /// Initializes EVMs.
    pub fn init_evms(&self, storage: Arc<dyn EthStorage>) -> NonEmpty<Box<dyn Evm>> {
        tracing::info!(evms = %self.num_evms, "starting evms");

        let mut evms: Vec<Box<dyn Evm>> = Vec::with_capacity(self.num_evms);
        for _ in 1..=self.num_evms {
            evms.push(Box::new(Revm::new(Arc::clone(&storage))));
        }
        NonEmpty::from_vec(evms).unwrap()
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
#[derive(Debug, Clone)]
pub enum Environment {
    /// Development environment (usually the developer's local machine).
    Development,
    /// Staging environment (usually a pre-production environment).
    Staging,
    /// Production environment (usually the live environment).
    Production,
    /// Any other environment not covered by previous options.
    Custom(String),
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
            s => Ok(Self::Custom(s.to_string())),
        }
    }
}
