use std::fmt;

use clap::Parser;
use clap::ValueEnum;

#[derive(Clone, Debug, ValueEnum)]
pub enum Storage {
    InMemory,
    Postgres,
}

impl fmt::Display for Storage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Storage::InMemory => write!(f, "in-memory"),
            Storage::Postgres => write!(f, "postgres"),
        }
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Config {
    /// Which storage to use
    #[arg(short, long, default_value_t = Storage::InMemory)]
    pub storage: Storage,

    #[arg(short, long, default_value = "postgres://localhost:5432")]
    pub database_url: String,
}
