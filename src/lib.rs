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

pub struct GlobalServices<T>
where
    T: clap::Parser + WithCommonConfig + Debug,
{
    pub config: T,
    _sentry_guard: Option<ClientInitGuard>,
    pub runtime: Runtime,
}

impl<T> GlobalServices<T>
where
    T: clap::Parser + WithCommonConfig + Debug,
{
    /// Executes global services initialization.
    pub fn init_global_services() -> Self
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

        let _sentry_guard = config.common().sentry_url.as_ref().map(|sentry_url| infra::init_sentry(sentry_url));

        let runtime = config.common().init_runtime();

        runtime.block_on(async { infra::init_tracing(config.common().tracing_url.as_ref()) });

        Self {
            config,
            _sentry_guard,
            runtime,
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
