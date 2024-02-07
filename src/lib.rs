use clap::Parser;
use config::Config;

pub mod config;
pub mod eth;
pub mod ext;
pub mod infra;

/// Executes global services initialization.
pub fn init_global_services() -> Config {
    let config = Config::parse();
    infra::init_tracing();
    infra::init_metrics();
    config
}
