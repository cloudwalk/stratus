//! Application configuration.

use std::net::SocketAddr;
use std::str::FromStr;

use clap::Parser;

/// Application configuration entry-point.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Config {
    /// Storage implementation.
    #[arg(short = 's', long = "storage", env = "STORAGE", default_value_t = StorageConfig::InMemory)]
    pub storage: StorageConfig,

    /// JSON-RPC binding address.
    #[arg(short = 'a', long = "address", env = "ADDRESS", default_value = "0.0.0.0:3000")]
    pub address: SocketAddr,
}

/// Storage configuration.
#[derive(Clone, Debug, strum::Display)]
pub enum StorageConfig {
    #[strum(serialize = "inmemory")]
    InMemory,

    #[strum(serialize = "postgres")]
    Postgres { url: String },
}

impl FromStr for StorageConfig {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "inmemory" => Ok(Self::InMemory),
            s if s.starts_with("postgres://") => Ok(Self::Postgres { url: s.to_string() }),
            s => Err(format!("unknown storage: {}", s)),
        }
    }
}
