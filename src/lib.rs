pub mod config;
pub mod eth;
pub mod ext;
pub mod infra;

/// Executes global services initialization.
pub fn init_global_services<T>() -> T
where
    T: clap::Parser,
{
    let config = T::parse();
    infra::init_tracing();
    infra::init_metrics();
    config
}
