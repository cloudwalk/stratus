use std::fmt::Debug;

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
    // load .env files
    let binary_full_path = std::env::current_exe().unwrap();
    let mut binary = binary_full_path.file_name().unwrap().to_str().unwrap().to_owned();
    if binary.starts_with("test_") {
        binary = "test-int".to_string();
    }
    let env = std::env::var("ENV").unwrap_or_else(|_| "local".to_string());
    let env_filename = format!("config/{}.env.{}", binary, env);

    println!("Reading ENV file: {}", env_filename);
    let _ = dotenvy::from_filename(env_filename);

    // parse configuration
    let config = T::parse();
    println!("Parsed configuration: {:?}", config);

    // init services
    infra::init_tracing();
    infra::init_metrics(config.common().metrics_histogram_kind);

    config
}
