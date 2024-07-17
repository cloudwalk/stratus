//! Standard library extensions.

use std::time::Duration;

use anyhow::anyhow;
use jsonrpsee::types::SubscriptionId;
use tokio::select;
use tokio::signal::unix::signal;
use tokio::signal::unix::SignalKind;

use crate::infra::tracing::info_task_spawn;
use crate::log_and_err;
use crate::GlobalState;

// -----------------------------------------------------------------------------
// Language constructs
// -----------------------------------------------------------------------------

/// Ternary operator from [ternop](https://docs.rs/ternop/1.0.1/ternop/), but renamed.
#[macro_export]
macro_rules! if_else {
    ($condition: expr, $_true: expr, $_false: expr) => {
        if $condition {
            $_true
        } else {
            $_false
        }
    };
}

/// `not(something)` instead of `!something`.
#[inline(always)]
pub fn not(value: bool) -> bool {
    !value
}

/// Extracts only the basename of a Rust type instead of the full qualification.
pub fn type_basename<T>() -> &'static str {
    let name: &'static str = std::any::type_name::<T>();
    name.split("::").last().unwrap()
}

// -----------------------------------------------------------------------------
// From / TryFrom
// -----------------------------------------------------------------------------

/// Generates [`From`] implementation for a [newtype](https://doc.rust-lang.org/rust-by-example/generics/new_types.html) that delegates to the inner type [`From`].
#[macro_export]
macro_rules! gen_newtype_from {
    (self = $type:ty, other = $($source:ty),+) => {
        $(
            impl From<$source> for $type {
                fn from(value: $source) -> Self {
                    Self(value.into())
                }
            }
        )+
    };
}

/// Generates [`TryFrom`] implementation for a [newtype](https://doc.rust-lang.org/rust-by-example/generics/new_types.html) that delegates to the inner type [`TryFrom`].
#[macro_export]
macro_rules! gen_newtype_try_from {
    (self = $type:ty, other = $($source:ty),+) => {
        $(
            impl TryFrom<$source> for $type {
                type Error = anyhow::Error;
                fn try_from(value: $source) -> Result<Self, Self::Error> {
                    Ok(Self(value.try_into().map_err(|err| anyhow::anyhow!("{:?}", err))?))
                }
            }
        )+
    };
}

// -----------------------------------------------------------------------------
// Display
// -----------------------------------------------------------------------------

/// Allows to implement `to_string` for types that does not have it.
pub trait DisplayExt {
    /// `to_string` for types that does not have it implemented.
    fn to_string_ext(&self) -> String;
}

impl DisplayExt for std::time::Duration {
    fn to_string_ext(&self) -> String {
        humantime::Duration::from(*self).to_string()
    }
}

impl DisplayExt for SubscriptionId<'_> {
    fn to_string_ext(&self) -> String {
        match self {
            SubscriptionId::Num(value) => value.to_string(),
            SubscriptionId::Str(value) => value.to_string(),
        }
    }
}

// -----------------------------------------------------------------------------
// Option
// -----------------------------------------------------------------------------

/// Extensions for `Option<T>`.
pub trait OptionExt<T> {
    /// Converts the Option inner type to the inferred type.
    fn map_into<U: From<T>>(self) -> Option<U>;
}

impl<T> OptionExt<T> for Option<T> {
    fn map_into<U: From<T>>(self) -> Option<U> {
        self.map(Into::into)
    }
}

// -----------------------------------------------------------------------------
// Result
// -----------------------------------------------------------------------------

/// Extensions for `Result<T, E>`.
pub trait ResultExt<T, E> {
    /// Unwraps a result informing that this operation is expected to be infallible.
    fn expect_infallible(self) -> T;
}

impl<T> ResultExt<T, serde_json::Error> for Result<T, serde_json::Error>
where
    T: Sized,
{
    fn expect_infallible(self) -> T {
        if let Err(ref e) = self {
            tracing::error!(reason = ?e, "serde serialization/deserialization that should be infallible");
        }
        self.expect("serde serialization/deserialization that should be infallible")
    }
}

// -----------------------------------------------------------------------------
// Duration
// -----------------------------------------------------------------------------

/// Parses a duration specified using human-time notation or fallback to milliseconds.
pub fn parse_duration(s: &str) -> anyhow::Result<Duration> {
    // try millis
    let millis: Result<u64, _> = s.parse();
    if let Ok(millis) = millis {
        return Ok(Duration::from_millis(millis));
    }

    // try humantime
    if let Ok(parsed) = humantime::parse_duration(s) {
        return Ok(parsed);
    }

    // error
    Err(anyhow!("invalid duration format: {}", s))
}

// -----------------------------------------------------------------------------
// Channels
// -----------------------------------------------------------------------------

/// Reads a value from an async channel logging timeout at some predefined interval.
///
/// Expects the tokio mpsc receiver.
#[macro_export]
macro_rules! channel_read {
    ($rx: ident) => {
        $crate::channel_read_impl!($rx, timeout_ms: 2000)
    };
    ($rx: ident, $timeout_ms:expr) => {
        $crate::channel_read_impl!($rx, timeout_ms: $timeout_ms),
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! channel_read_impl {
    ($rx: ident, timeout_ms: $timeout: expr) => {{
        const TARGET: &str = const_format::formatcp!("{}::{}", module_path!(), "rx");
        const TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_millis($timeout);

        loop {
            match tokio::time::timeout(TIMEOUT, $rx.recv()).await {
                Ok(value) => break value,
                Err(_) => {
                    tracing::warn!(target: TARGET, channel = %stringify!($rx), timeout_ms = %TIMEOUT.as_millis(), "timeout reading channel");
                    continue;
                }
            }
        }
    }};
}

/// Reads a value from a sync channel logging timeout at some predefined interval.
///
/// Expects the `std` mpsc receiver.
#[macro_export]
macro_rules! channel_read_sync {
    ($rx: ident) => {
        $crate::channel_read_sync_impl!($rx, timeout_ms: 2000)
    };
    ($rx: ident, $timeout_ms:expr) => {
        $crate::channel_read_sync_impl!($rx, timeout_ms: $timeout_ms),
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! channel_read_sync_impl {
    ($rx: ident, timeout_ms: $timeout: expr) => {{
        const TARGET: &str = const_format::formatcp!("{}::{}", module_path!(), "rx");
        const TIMEOUT: std::time::Duration = std::time::Duration::from_millis($timeout);

        loop {
            match $rx.recv_timeout(TIMEOUT) {
                Ok(value) => break Ok(value),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    tracing::warn!(target: TARGET, channel = %stringify!($rx), timeout_ms = %TIMEOUT.as_millis(), "timeout reading channel");
                    continue;
                }
                e @ Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    break e
                }
            }
        }
    }};
}

// -----------------------------------------------------------------------------
// Tokio
// -----------------------------------------------------------------------------

/// Indicates why a sleep is happening.
#[derive(Debug, strum::Display)]
pub enum SleepReason {
    /// Task is executed at predefined intervals.
    #[strum(to_string = "interval")]
    Interval,

    /// Task is awaiting a backoff before retrying the operation.
    #[strum(to_string = "retry-backoff")]
    RetryBackoff,

    /// Task is awaiting an external system or component to produde or synchronize data.
    #[strum(to_string = "sync-data")]
    SyncData,
}

/// Sleeps the current task and tracks why it is sleeping.
#[cfg(feature = "tracing")]
#[inline(always)]
pub async fn traced_sleep(duration: Duration, reason: SleepReason) {
    use tracing::Instrument;

    let span = tracing::debug_span!("tokio::sleep", duration_ms = %duration.as_millis(), %reason);
    async {
        tracing::debug!(duration_ms = %duration.as_millis(), %reason, "sleeping");
        tokio::time::sleep(duration).await;
    }
    .instrument(span)
    .await;
}

#[cfg(not(feature = "tracing"))]
#[inline(always)]
pub async fn traced_sleep(duration: Duration, _: SleepReason) {
    tokio::time::sleep(duration).await;
}

/// Spawns an async Tokio task with a name to be displayed in tokio-console.
#[track_caller]
pub fn spawn_named<T>(name: &str, task: impl std::future::Future<Output = T> + Send + 'static) -> tokio::task::JoinHandle<T>
where
    T: Send + 'static,
{
    info_task_spawn(name);

    tokio::task::Builder::new()
        .name(name)
        .spawn(task)
        .expect("spawning named async task should not fail")
}

/// Spawns a blocking Tokio task with a name to be displayed in tokio-console.
#[track_caller]
pub fn spawn_blocking_named<T>(name: &str, task: impl FnOnce() -> T + Send + 'static) -> tokio::task::JoinHandle<T>
where
    T: Send + 'static,
{
    info_task_spawn(name);

    tokio::task::Builder::new()
        .name(name)
        .spawn_blocking(task)
        .expect("spawning named blocking task should not fail")
}

/// Spawns a thread with the given name. Thread has access to Tokio current runtime.
#[track_caller]
pub fn spawn_thread<T>(name: &str, task: impl FnOnce() -> T + Send + 'static) -> std::thread::JoinHandle<T>
where
    T: Send + 'static,
{
    info_task_spawn(name);

    let runtime = tokio::runtime::Handle::current();
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            let _runtime_guard = runtime.enter();
            task()
        })
        .expect("spawning background thread should not fail")
}

/// Spawns a handler that listens to system signals.
pub async fn spawn_signal_handler() -> anyhow::Result<()> {
    const TASK_NAME: &str = "signal-handler";

    let mut sigterm = match signal(SignalKind::terminate()) {
        Ok(signal) => signal,
        Err(e) => return log_and_err!(reason = e, "failed to init SIGTERM watcher"),
    };
    let mut sigint = match signal(SignalKind::interrupt()) {
        Ok(signal) => signal,
        Err(e) => return log_and_err!(reason = e, "failed to init SIGINT watcher"),
    };

    spawn_named("sys::signal_handler", async move {
        select! {
            _ = sigterm.recv() => {
                GlobalState::shutdown_from(TASK_NAME, "received SIGTERM");
            }
            _ = sigint.recv() => {
                GlobalState::shutdown_from(TASK_NAME, "received SIGINT");
            }
        }
    });

    Ok(())
}

// -----------------------------------------------------------------------------
// serde_json
// -----------------------------------------------------------------------------

pub type JsonValue = serde_json::Value;

/// Serializes any serializable value to non-formatted [`String`] without having to check for errors.
pub fn to_json_string<V: serde::Serialize>(value: &V) -> String {
    serde_json::to_string(value).expect_infallible()
}

/// Serializes any serializable value to formatted [`String`] without having to check for errors.
pub fn to_json_string_pretty<V: serde::Serialize>(value: &V) -> String {
    serde_json::to_string_pretty(value).expect_infallible()
}

/// Serializes any serializable value to [`serde_json::Value`] without having to check for errors.
pub fn to_json_value<V: serde::Serialize>(value: V) -> serde_json::Value {
    serde_json::to_value(value).expect_infallible()
}

/// Deserializes any deserializable value from [`&str`] without having to check for errors.
pub fn from_json_str<T: serde::de::DeserializeOwned>(s: &str) -> T {
    serde_json::from_str::<T>(s).expect_infallible()
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

/// Generates unit test that checks implementation of [`Serialize`](serde::Serialize) and [`Deserialize`](serde::Deserialize) are compatible.
#[macro_export]
macro_rules! gen_test_serde {
    ($type:ty) => {
        paste::paste! {
            #[test]
            pub fn [<serde_ $type:snake>]() {
                use $crate::ext::ResultExt;

                let value = <fake::Faker as fake::Fake>::fake::<$type>(&fake::Faker);
                let json = serde_json::to_string(&value).expect_infallible();
                assert_eq!(serde_json::from_str::<$type>(&json).unwrap(), value);
            }
        }
    };
}
