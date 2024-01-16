//! Application configuration.

use std::{str::FromStr, net::SocketAddr};

use clap::Parser;

/// Application configuration entry-point.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Config {
    /// Main storage to use.
    #[arg(short, long, default_value_t = StorageConfig::InMemory)]
    pub storage: StorageConfig,

    /// The IP Address to host the RPC Server
    #[arg(short, long, default_value = "0.0.0.0:3000")]
    pub rpc_address: SocketAddr,
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
