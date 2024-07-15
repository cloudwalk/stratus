use std::env;
use std::fmt::Debug;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use once_cell::sync::Lazy;
use sentry::ClientInitGuard;
use tokio::runtime::Runtime;
use tokio_util::sync::CancellationToken;

use crate::config::load_dotenv;
use crate::config::WithCommonConfig;
use crate::eth::Consensus;
use crate::ext::spawn_signal_handler;
use crate::infra;
use crate::infra::tracing::warn_task_cancellation;

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
    pub fn init() -> Self
    where
        T: clap::Parser + WithCommonConfig + Debug,
    {
        // translate renamed environment variables because clap does not support multiple aliases for env-vars
        if let Ok(value) = env::var("TRACING_COLLECTOR_URL") {
            env::set_var("TRACING_URL", value);
        }
        if let Ok(value) = env::var("LOG_FORMAT") {
            env::set_var("TRACING_LOG_FORMAT", value);
        }

        // parse configuration
        load_dotenv();
        let config = T::parse();
        let common = config.common();

        // init tokio
        let runtime = common.init_runtime().expect("failed to init tokio runtime");

        // init tracing
        runtime
            .block_on(infra::init_tracing(&common.tracing, common.sentry_url.as_deref(), common.tokio_console_address))
            .expect("failed to init tracing");

        // init metrics
        infra::init_metrics(common.metrics_exporter_address).expect("failed to init metrics");

        // init sentry
        let sentry_guard = common
            .sentry_url
            .as_ref()
            .map(|sentry_url| infra::init_sentry(sentry_url, common.env).expect("failed to init sentry"));

        // init signal handler
        runtime.block_on(spawn_signal_handler()).expect("failed to init signal handlers");

        Self {
            config,
            runtime,
            _sentry_guard: sentry_guard,
        }
    }
}

// -----------------------------------------------------------------------------
// Global state
// -----------------------------------------------------------------------------

// Stratus is running or being shut-down?
static STRATUS_SHUTDOWN: Lazy<CancellationToken> = Lazy::new(CancellationToken::new);

/// Miner should mine new blocks?
static MINER_ENABLED: AtomicBool = AtomicBool::new(true);

/// Unknown clients can interact with the application?
static UNKNOWN_CLIENT_ENABLED: AtomicBool = AtomicBool::new(true);

pub struct GlobalState;

impl GlobalState {
    // -------------------------------------------------------------------------
    // Shutdown
    // -------------------------------------------------------------------------

    /// Shutdown the application.
    ///
    /// Returns the formatted reason for shutdown.
    pub fn shutdown_from(caller: &str, reason: &str) -> String {
        tracing::warn!(%caller, %reason, "application is shutting down");
        STRATUS_SHUTDOWN.cancel();
        format!("{} {}", caller, reason)
    }

    /// Checks if the application is being shutdown.
    pub fn is_shutdown() -> bool {
        STRATUS_SHUTDOWN.is_cancelled()
    }

    /// Checks if the application is being shutdown. Emits an warning with the task name in case it is.
    pub fn is_shutdown_warn(task_name: &str) -> bool {
        let shutdown = Self::is_shutdown();
        if shutdown {
            warn_task_cancellation(task_name);
        }
        shutdown
    }

    /// Waits until a shutdown is signalled.
    pub async fn wait_shutdown() {
        STRATUS_SHUTDOWN.cancelled().await;
    }

    /// Waits until a shutdown is signalled. Emits an warning with the task name when it is.
    pub async fn wait_shutdown_warn(task_name: &str) {
        Self::wait_shutdown().await;
        warn_task_cancellation(task_name);
    }

    // -------------------------------------------------------------------------
    // Leadership
    // -------------------------------------------------------------------------

    /// Checks if node is leader.
    pub fn is_leader() -> bool {
        Consensus::is_leader()
    }

    // -------------------------------------------------------------------------
    // Miner
    // -------------------------------------------------------------------------

    /// Enables or disables the miner.
    pub fn set_miner_enabled(enabled: bool) {
        MINER_ENABLED.store(enabled, Ordering::Relaxed);
    }

    /// Checks if the miner is enabled.
    pub fn is_miner_enabled() -> bool {
        MINER_ENABLED.load(Ordering::Relaxed)
    }

    // -------------------------------------------------------------------------
    // Unknown Client
    // -------------------------------------------------------------------------

    /// Enables or disables the unknown client.
    pub fn set_unknown_client_enabled(enabled: bool) {
        UNKNOWN_CLIENT_ENABLED.store(enabled, Ordering::Relaxed);
    }

    /// Checks if the unknown client is enabled.
    pub fn is_unknown_client_enabled() -> bool {
        UNKNOWN_CLIENT_ENABLED.load(Ordering::Relaxed)
    }
}
