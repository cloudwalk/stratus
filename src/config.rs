//! Application configuration.

use std::str::FromStr;

use clap::Parser;

/// Application configuration entry-point.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Config {
    /// Main storage to use.
    #[arg(short, long, default_value_t = StorageConfig::InMemory)]
    pub storage: StorageConfig,
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
