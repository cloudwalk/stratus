use crate::config::WithCommonConfig;

pub mod config;
pub mod eth;
pub mod ext;
pub mod infra;

/// Executes global services initialization.
pub fn init_global_services<T>() -> T
where
    T: clap::Parser + WithCommonConfig,
{
    let config = T::parse();
    infra::init_tracing();
    infra::init_metrics(config.common().metrics_histogram_kind);
    config
}
