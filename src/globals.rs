use std::fmt::Debug;
use std::sync::LazyLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use chrono::DateTime;
use chrono::Utc;
use parking_lot::Mutex;
use sentry::ClientInitGuard;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use tokio::runtime::Runtime;
use tokio::sync::Semaphore;
use tokio::sync::watch::Sender;
use tokio_util::sync::CancellationToken;

use crate::alias::JsonValue;
use crate::config;
use crate::config::StratusConfig;
use crate::config::WithCommonConfig;
use crate::eth::rpc::RpcContext;
use crate::ext::not;
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
    #[allow(clippy::expect_used)]
    /// Executes global services initialization.
    pub fn init() -> Self
    where
        T: clap::Parser + WithCommonConfig + Debug,
    {
        GlobalState::setup_start_time();

        // env-var support
        config::load_dotenv_file();
        config::load_env_aliases();

        // parse configuration
        let config = T::parse();
        let common = config.common();

        // Set the unknown_client_enabled value
        GlobalState::set_unknown_client_enabled(common.unknown_client_enabled);

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

#[derive(Clone, Copy, PartialEq, Eq, Debug, strum::Display)]
pub enum NodeMode {
    #[strum(to_string = "leader")]
    Leader,

    #[strum(to_string = "follower")]
    Follower,

    /// Fake leader feches a block, re-executes its txs and then mines it's own block.
    #[strum(to_string = "fake-leader")]
    FakeLeader,
}

// -----------------------------------------------------------------------------
// Global state
// -----------------------------------------------------------------------------

pub static STRATUS_SHUTDOWN_SIGNAL: LazyLock<CancellationToken> = LazyLock::new(CancellationToken::new);

/// Importer is running or being shut-down?
static IMPORTER_SHUTDOWN: AtomicBool = AtomicBool::new(true);

/// A guard that is taken when importer is running.
pub static IMPORTER_ONLINE_TASKS_SEMAPHORE: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(crate::eth::follower::importer::TASKS_COUNT));

/// Transaction should be accepted?
static TRANSACTIONS_ENABLED: AtomicBool = AtomicBool::new(true);

/// Unknown clients can interact with the application?
static UNKNOWN_CLIENT_ENABLED: AtomicBool = AtomicBool::new(true);

/// Current node mode.
static NODE_MODE: Mutex<NodeMode> = Mutex::new(NodeMode::Follower);

static START_TIME: LazyLock<DateTime<Utc>> = LazyLock::new(Utc::now);

/// Is stratus healthy?
static HEALTH: LazyLock<Sender<bool>> = LazyLock::new(|| tokio::sync::watch::Sender::new(false));

/// Should stratus restart when unhealthy?
static RESTART_ON_UNHEALTHY: AtomicBool = AtomicBool::new(true);

#[derive(Serialize, Deserialize, Debug)]
pub struct GlobalState;

impl GlobalState {
    // -------------------------------------------------------------------------
    // Application Shutdown
    // -------------------------------------------------------------------------

    pub fn set_health(new_health: bool) {
        HEALTH.send_if_modified(|health| {
            if *health != new_health {
                tracing::info!(?new_health, "health status updated");
                *health = new_health;
                true
            } else {
                false
            }
        });
    }

    pub fn is_healthy() -> bool {
        *HEALTH.borrow()
    }

    pub fn get_health_receiver() -> tokio::sync::watch::Receiver<bool> {
        HEALTH.subscribe()
    }

    pub fn restart_on_unhealthy() -> bool {
        RESTART_ON_UNHEALTHY.load(Ordering::Relaxed)
    }

    pub fn set_restart_on_unhealthy(state: bool) {
        RESTART_ON_UNHEALTHY.store(state, Ordering::Relaxed);
    }

    /// Shutdown the application.
    ///
    /// Returns the formatted reason for shutdown.
    pub fn shutdown_from(caller: &str, reason: &str) -> String {
        tracing::warn!(%caller, %reason, "application is shutting down");
        STRATUS_SHUTDOWN_SIGNAL.cancel();
        format!("{caller} {reason}")
    }

    /// Checks if the application is being shutdown.
    pub fn is_shutdown() -> bool {
        STRATUS_SHUTDOWN_SIGNAL.is_cancelled()
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
        STRATUS_SHUTDOWN_SIGNAL.cancelled().await;
    }

    /// Waits until a shutdown is signalled. Emits an warning with the task name when it is.
    pub async fn wait_shutdown_warn(task_name: &str) {
        Self::wait_shutdown().await;
        warn_task_cancellation(task_name);
    }

    // -------------------------------------------------------------------------
    // Importer Shutdown
    // -------------------------------------------------------------------------

    /// Shutdown the importer.
    ///
    /// Returns the formatted reason for importer shutdown.
    pub fn shutdown_importer_from(caller: &str, reason: &str) -> String {
        tracing::warn!(%caller, %reason, "importer is shutting down");
        Self::set_importer_shutdown(true);
        format!("{caller} {reason}")
    }

    /// Checks if the importer is shut down or being shut down.
    pub fn is_importer_shutdown() -> bool {
        IMPORTER_SHUTDOWN.load(Ordering::Relaxed)
    }

    /// Waits till importer is done.
    pub async fn wait_for_importer_to_finish() {
        // 3 permits will be available when all 3 tasks are finished
        let result = IMPORTER_ONLINE_TASKS_SEMAPHORE
            .acquire_many(crate::eth::follower::importer::TASKS_COUNT as u32)
            .await;

        if let Err(e) = result {
            tracing::error!(reason = ?e, "error waiting for importer to finish");
        }
    }

    /// Checks if the importer is being shutdown. Emits a warning with the task name in case it is.
    pub fn is_importer_shutdown_warn(task_name: &str) -> bool {
        let shutdown = Self::is_importer_shutdown();
        if shutdown {
            warn_task_cancellation(task_name);
        }
        shutdown
    }

    pub fn set_importer_shutdown(shutdown: bool) {
        IMPORTER_SHUTDOWN.store(shutdown, Ordering::Relaxed);
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
        let StratusConfig {
            follower, leader, fake_leader, ..
        } = config;

        let mode = match (follower, leader, fake_leader) {
            (true, false, false) => NodeMode::Follower,
            (false, true, false) => NodeMode::Leader,
            (false, false, true) => NodeMode::FakeLeader,
            _ => unreachable!("exactly one must be true, config should be checked by clap"),
        };
        Self::set_node_mode(mode);

        let should_run_importer = mode != NodeMode::Leader;
        Self::set_importer_shutdown(not(should_run_importer));
    }

    pub fn set_node_mode(mode: NodeMode) {
        *NODE_MODE.lock() = mode;
    }

    pub fn get_node_mode() -> NodeMode {
        *NODE_MODE.lock()
    }

    // -------------------------------------------------------------------------
    // JSON State
    // -------------------------------------------------------------------------

    pub fn get_global_state_as_json(ctx: &RpcContext) -> JsonValue {
        let start_time = Self::get_start_time();
        let elapsed_time = {
            let delta = start_time.signed_duration_since(Utc::now()).abs();
            let seconds = delta.num_seconds() % 60;
            let minutes = delta.num_minutes() % 60;
            let hours = delta.num_hours() % 24;
            let days = delta.num_days();
            format!("{days} days and {hours:02}:{minutes:02}:{seconds:02} elapsed")
        };

        json!({
            "is_leader": Self::get_node_mode() == NodeMode::Leader || Self::get_node_mode() == NodeMode::FakeLeader,
            "is_shutdown": Self::is_shutdown(),
            "is_importer_shutdown": Self::is_importer_shutdown(),
            "is_interval_miner_running": ctx.server.miner.is_interval_miner_running(),
            "transactions_enabled": Self::is_transactions_enabled(),
            "miner_paused": ctx.server.miner.is_paused(),
            "unknown_client_enabled": Self::is_unknown_client_enabled(),
            "start_time": start_time.format("%d/%m/%Y %H:%M UTC").to_string(),
            "elapsed_time": elapsed_time,
        })
    }

    fn get_start_time() -> DateTime<Utc> {
        *START_TIME
    }

    pub fn setup_start_time() {
        LazyLock::force(&START_TIME);
    }
}
