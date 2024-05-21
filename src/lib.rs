use std::env;
use std::fmt::Debug;

use sentry::ClientInitGuard;
use tokio::runtime::Runtime;

use crate::config::load_dotenv;
use crate::config::WithCommonConfig;

pub mod config;
pub mod eth;
pub mod ext;
pub mod infra;
pub mod utils;

pub struct GlobalServices<T>
where
    T: clap::Parser + WithCommonConfig + Debug,
{
    pub config: T,
    pub runtime: Runtime,
    _sentry_guard: Option<ClientInitGuard>,
}

impl<T> GlobalServices<T>
where
    T: clap::Parser + WithCommonConfig + Debug,
{
    /// Executes global services initialization.
    pub fn init() -> Self
    where
        T: clap::Parser + WithCommonConfig + Debug,
    {
        // parse configuration
        load_dotenv();
        let config = T::parse();

        if env::var_os("PERM_STORAGE_CONNECTIONS").is_some_and(|value| value == "1") {
            println!("WARNING: env var PERM_STORAGE_CONNECTIONS is set to 1, if it cause connection problems, try increasing it");
        }

        // init metrics
        #[cfg(feature = "metrics")]
        infra::init_metrics(config.common().metrics_histogram_kind);

        // init sentry
        let _sentry_guard = config.common().sentry_url.as_ref().map(|sentry_url| infra::init_sentry(sentry_url));

        // init tokio
        let runtime = config.common().init_runtime();

        // init tracing
        runtime.block_on(infra::init_tracing(config.common().tracing_url.as_ref()));

        Self {
            config,
            runtime,
            _sentry_guard,
        }
    }
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
