use std::env;
use std::fmt::Debug;

use sentry::ClientInitGuard;

use crate::config::load_dotenv;
use crate::config::WithCommonConfig;

pub mod config;
pub mod eth;
pub mod ext;
pub mod infra;

/// Executes global services initialization.
pub fn init_global_services<T>() -> (T, Option<ClientInitGuard>)
where
    T: clap::Parser + WithCommonConfig + Debug,
{
    // parse configuration
    load_dotenv();
    let config = T::parse();
    println!("parsed configuration: {:?}", config);

    if env::var_os("PERM_STORAGE_CONNECTIONS").is_some_and(|value| value == "1") {
        println!("WARNING: env var PERM_STORAGE_CONNECTIONS is set to 1, if it cause connection problems, try increasing it");
    }

    // init services
    #[cfg(feature = "metrics")]
    infra::init_metrics(config.common().metrics_histogram_kind);

    let guard = config.common().sentry_url.as_ref().map(|sentry_url| infra::init_sentry(sentry_url));

    (config, guard)
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
