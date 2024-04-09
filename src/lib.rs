use std::fmt::Debug;

use crate::config::load_dotenv;
use crate::config::WithCommonConfig;

pub mod config;
pub mod eth;
pub mod ext;
pub mod infra;

/// Executes global services initialization.
pub fn init_global_services<T>() -> T
where
    T: clap::Parser + WithCommonConfig + Debug,
{
    // parse configuration
    load_dotenv();
    let config = T::parse();
    println!("parsed configuration: {:?}", config);

    // init services
    infra::init_tracing();
    #[cfg(feature = "metrics")]
    infra::init_metrics(config.common().metrics_histogram_kind);

    config
}

/// Get the current binary basename.
pub fn bin_name() -> String {
    let binary = std::env::current_exe().unwrap();
    let binary_basename = binary.file_name().unwrap().to_str().unwrap().to_lowercase();

    if binary_basename.starts_with("test_") {
        "tests".to_string()
    } else {
        binary_basename
    }
}
