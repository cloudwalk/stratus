use std::fmt::Debug;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use once_cell::sync::Lazy;
use sentry::ClientInitGuard;
use tokio::runtime::Runtime;
use tokio_util::sync::CancellationToken;

use crate::config;
use crate::config::StratusConfig;
use crate::config::WithCommonConfig;
use crate::ext::spawn_signal_handler;
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
        // env-var support
        config::load_dotenv_file();
        config::load_env_aliases();

        // parse configuration
        let config = T::parse();
        let common = config.common();

        // init tokio
        let tokio = common.init_tokio_runtime().expect("failed to init tokio runtime");

        // init tracing
        tokio.block_on(async {
            common.tracing.init(&common.sentry).expect("failed to init tracing");
        });

        // init observability services
        common.metrics.init().expect("failed to init metrics");

        // init sentry
        let sentry_guard = common
            .sentry
            .as_ref()
            .map(|sentry_config| sentry_config.init(common.env).expect("failed to init sentry"));

        // init signal handler
        tokio.block_on(spawn_signal_handler()).expect("failed to init signal handlers");

        Self {
            config,
            runtime: tokio,
            _sentry_guard: sentry_guard,
        }
    }
}

// -----------------------------------------------------------------------------
// Node mode
// -----------------------------------------------------------------------------

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, strum::Display, strum::EnumString)]
pub enum NodeMode {
    #[strum(to_string = "leader")]
    Leader,

    #[strum(to_string = "follower")]
    Follower,
}

// -----------------------------------------------------------------------------
// Global state
// -----------------------------------------------------------------------------

// Stratus is running or being shut-down?
static STRATUS_SHUTDOWN: Lazy<CancellationToken> = Lazy::new(CancellationToken::new);

/// Transaction should be accepted?
static TRANSACTIONS_ENABLED: AtomicBool = AtomicBool::new(true);

/// Miner should mine new blocks?
static MINER_ENABLED: AtomicBool = AtomicBool::new(true);

/// Unknown clients can interact with the application?
static UNKNOWN_CLIENT_ENABLED: AtomicBool = AtomicBool::new(true);

/// Current node mode.
static IS_LEADER: AtomicBool = AtomicBool::new(false);

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
    // Transaction
    // -------------------------------------------------------------------------

    /// Sets whether transactions should be accepted.
    pub fn set_transactions_enabled(enabled: bool) {
        TRANSACTIONS_ENABLED.store(enabled, Ordering::Relaxed);
    }

    /// Checks if transactions are enabled.
    pub fn is_transactions_enabled() -> bool {
        TRANSACTIONS_ENABLED.load(Ordering::Relaxed)
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

    // -------------------------------------------------------------------------
    // Node Mode
    // -------------------------------------------------------------------------

    /// Initializes the node mode based on the StratusConfig.
    pub fn initialize_node_mode(config: &StratusConfig) {
        let mode = if config.follower { NodeMode::Follower } else { NodeMode::Leader };
        Self::set_node_mode(mode);
    }

    /// Sets the current node mode.
    pub fn set_node_mode(mode: NodeMode) {
        IS_LEADER.store(matches!(mode, NodeMode::Leader), Ordering::Relaxed);
    }

    /// Gets the current node mode.
    pub fn get_node_mode() -> NodeMode {
        if IS_LEADER.load(Ordering::Relaxed) {
            NodeMode::Leader
        } else {
            NodeMode::Follower
        }
    }

    /// Checks if the node is in follower mode.
    pub fn is_follower() -> bool {
        !IS_LEADER.load(Ordering::Relaxed)
    }

    /// Checks if the node is in leader mode.
    pub fn is_leader() -> bool {
        IS_LEADER.load(Ordering::Relaxed)
    }
}
