use std::env;
use std::fmt::Debug;
use std::fmt::Display;

use once_cell::sync::Lazy;
use sentry::ClientInitGuard;
use tokio::runtime::Runtime;
use tokio_util::sync::CancellationToken;

use crate::config::load_dotenv;
use crate::config::WithCommonConfig;
use crate::infra;
use crate::infra::tracing::warn_task_cancellation;
use crate::utils::spawn_signal_handler;

// -----------------------------------------------------------------------------
// Global services
// -----------------------------------------------------------------------------

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
    pub fn init() -> anyhow::Result<Self>
    where
        T: clap::Parser + WithCommonConfig + Debug,
    {
        // parse configuration
        load_dotenv();
        let config = T::parse();

        if env::var_os("PERM_STORAGE_CONNECTIONS").is_some_and(|value| value == "1") {
            println!("WARNING: env var PERM_STORAGE_CONNECTIONS is set to 1, if it cause connection problems, try increasing it");
        }

        // init tokio
        let runtime = config.common().init_runtime();

        // init metrics
        #[cfg(feature = "metrics")]
        infra::init_metrics(config.common().metrics_histogram_kind);

        // init sentry
        let _sentry_guard = config.common().sentry_url.as_ref().map(|sentry_url| infra::init_sentry(sentry_url));

        // init signal handler
        runtime.block_on(spawn_signal_handler())?;

        // init tracing
        runtime.block_on(infra::init_tracing(config.common().tracing_url.as_ref()));

        Ok(Self {
            config,
            runtime,
            _sentry_guard,
        })
    }
}

// -----------------------------------------------------------------------------
// Global state
// -----------------------------------------------------------------------------

static CANCELLATION: Lazy<CancellationToken> = Lazy::new(CancellationToken::new);
pub struct GlobalState;

impl GlobalState {
    /// Shutdown the application, displaying the reason.
    ///
    /// Returns the formatted reason for shutdown.
    pub fn shutdown_from(caller: &str, reason: &str) -> String {
        tracing::warn!(%caller, %reason, "application is shutting down");
        CANCELLATION.cancel();
        format!("{} {}", caller, reason)
    }

    /// Shutdown the application, displaying the reason and error.
    ///
    /// Returns the formatted reason for shutdown.
    pub fn shutdown_with_caller_and_err(caller: &str, reason: &str, error: impl Display) -> String {
        tracing::warn!(%caller, %reason, %error, "application is shutting down");
        CANCELLATION.cancel();
        format!("{} {}: {}", caller, reason, error)
    }

    /// Checks if the application is being shutdown.
    pub fn is_shutdown() -> bool {
        CANCELLATION.is_cancelled()
    }

    /// Checks if the application is being shutdown. Emits an warning with the task name in case it is.
    pub fn warn_if_shutdown(task_name: &str) -> bool {
        let shutdown = Self::is_shutdown();
        if shutdown {
            warn_task_cancellation(task_name);
        }
        shutdown
    }

    /// Awaits until a shutdown is received.
    pub async fn until_shutdown() {
        CANCELLATION.cancelled().await;
    }
}
