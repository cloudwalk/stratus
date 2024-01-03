use clap::{Parser, ValueEnum};
use std::fmt;

#[derive(Clone, Debug, ValueEnum)]
pub enum Storage {
    InMemory,
    Redis,
}

impl fmt::Display for Storage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Storage::InMemory => write!(f, "InMemoryStorage"),
            Storage::Redis => write!(f, "RedisStorage"),
        }
    }
}

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
pub struct Config {
    // The storage to use
    #[arg(long, default_value = "InMemoryStorage")]
    pub storage: Storage,

    // The URL of the Redis server
    #[arg(long, default_value = "redis://127.0.0.1:6379")]
    pub redis_url: String,
}