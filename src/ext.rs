//! Standard library extensions.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::time::Duration;

use anyhow::anyhow;
use chrono::DateTime;
use chrono::Utc;
use jsonrpsee::types::SubscriptionId;
use serde::Serialize;
use serde::Serializer;
use tokio::select;
use tokio::signal::unix::signal;
use tokio::signal::unix::SignalKind;
use tokio::sync::watch::error::RecvError;

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
    name.rsplit("::").next().unwrap_or(name)
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

pub trait InfallibleExt<T, E> {
    /// Unwraps a result informing that this operation is expected to be infallible.
    fn expect_infallible(self) -> T;
}

impl<T> InfallibleExt<T, serde_json::Error> for Result<T, serde_json::Error>
where
    T: Sized,
{
    #[allow(clippy::expect_used)]
    fn expect_infallible(self) -> T {
        if let Err(ref e) = self {
            tracing::error!(reason = ?e, "expected infallible serde serialization/deserialization");
        }
        self.expect("serde serialization/deserialization")
    }
}

impl InfallibleExt<DateTime<Utc>, ()> for Option<DateTime<Utc>> {
    #[allow(clippy::expect_used)]
    fn expect_infallible(self) -> DateTime<Utc> {
        if self.is_none() {
            tracing::error!("expected infallible datetime conversion");
        }
        self.expect("infallible datetime conversion")
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
#[allow(clippy::expect_used)]
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

/// Spawns a thread with the given name. Thread has access to Tokio current runtime.
#[allow(clippy::expect_used)]
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

/// Serializes any serializable value to non-formatted [`String`] without having to check for errors.
pub fn to_json_string<V: serde::Serialize>(value: &V) -> String {
    serde_json::to_string(value).expect_infallible()
}

/// Serializes any serializable value to [`serde_json::Value`] without having to check for errors.
pub fn to_json_value<V: serde::Serialize>(value: V) -> serde_json::Value {
    serde_json::to_value(value).expect_infallible()
}

/// Deserializes any deserializable value from [`&str`] without having to check for errors.
pub fn from_json_str<T: serde::de::DeserializeOwned>(s: &str) -> T {
    serde_json::from_str::<T>(s).expect_infallible()
}

pub fn ordered_map<S, K: Ord + Serialize, V: Serialize>(value: &HashMap<K, V>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let ordered: BTreeMap<_, _> = value.iter().collect();
    ordered.serialize(serializer)
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
            pub fn [<serde_debug_json_ $type:snake>]() {
                let original = <fake::Faker as fake::Fake>::fake::<$type>(&fake::Faker);
                let encoded_json = serde_json::to_string(&original).expect(concat!("failed to serialize in test for ", stringify!($type)));
                let encoded_debug = format!("{:?}", original);
                assert_eq!(encoded_json, encoded_debug);
            }

            #[test]
            pub fn [<serde_json_ $type:snake>]() {
                // encode
                let original = <fake::Faker as fake::Fake>::fake::<$type>(&fake::Faker);
                let encoded = serde_json::to_string(&original).unwrap();

                // decode
                let decoded = serde_json::from_str::<$type>(&encoded).unwrap();
                assert_eq!(decoded, original);

                // re-encode
                let reencoded = serde_json::to_string(&decoded).unwrap();
                assert_eq!(reencoded, encoded);

                // re-decode
                let redecoded = serde_json::from_str::<$type>(&reencoded).unwrap();
                assert_eq!(redecoded, original);
            }
        }
    };
}

/// Generates a unit test that verifies JSON serialization/deserialization compatibility using snapshots.
#[macro_export]
macro_rules! gen_test_json {
    ($type:ty) => {
        paste::paste! {
            #[test]
            fn [<test_ $type:snake _json_snapshot>]() -> anyhow::Result<()> {
                use anyhow::bail;
                use std::path::Path;
                use std::{env, fs};

                let expected: $type = $crate::utils::test_utils::fake_first::<$type>();
                let expected_json = serde_json::to_string_pretty(&expected)?;
                let snapshot_parent_path = "tests/fixtures/primitives";
                let snapshot_name = format!("{}.json", stringify!($type));
                let snapshot_path = format!("{}/{}", snapshot_parent_path, snapshot_name);

                // WARNING: If you need to update snapshots (DANGEROUS_UPDATE_SNAPSHOTS=1), you have likely
                // broken backwards compatibility! Make sure this is intentional.
                if !Path::new(&snapshot_path).exists() {
                    if env::var("DANGEROUS_UPDATE_SNAPSHOTS").is_ok() {
                        fs::create_dir_all(snapshot_parent_path)?;
                        fs::write(&snapshot_path, &expected_json)?;
                    } else {
                        bail!("snapshot file at '{snapshot_path}' doesn't exist and DANGEROUS_UPDATE_SNAPSHOTS is not set");
                    }
                }

                // Read and attempt to deserialize the snapshot
                let snapshot_content = fs::read_to_string(&snapshot_path)?;
                let deserialized = match serde_json::from_str::<$type>(&snapshot_content) {
                    Ok(value) => value,
                    Err(e) => {
                        bail!("Failed to deserialize snapshot:\nError: {}\n\nExpected JSON:\n{}\n\nActual JSON from snapshot:\n{}",
                            e, expected_json, snapshot_content);
                    }
                };

                // Compare the values
                assert_eq!(
                    expected, deserialized,
                    "\nDeserialized value doesn't match expected:\n\nExpected JSON:\n{}\n\nDeserialized JSON:\n{}",
                    expected_json,
                    serde_json::to_string_pretty(&deserialized)?
                );

                Ok(())
            }
        }
    };
}

/// Generates unit test that checks that bincode's serialization and deserialization are compatible
#[macro_export]
macro_rules! gen_test_bincode {
    ($type:ty) => {
        paste::paste! {
            #[test]
            pub fn [<bincode_ $type:snake>]() {
                let value = <fake::Faker as fake::Fake>::fake::<$type>(&fake::Faker);
                let binary = bincode::serialize(&value).unwrap();
                assert_eq!(bincode::deserialize::<$type>(&binary).unwrap(), value);
            }
        }
    };
}

pub trait WatchReceiverExt<T> {
    #[allow(async_fn_in_trait)]
    async fn wait_for_change(
        &mut self,
        f: impl Fn(&T) -> bool,
    ) -> Result<(), RecvError>;
}

impl<T> WatchReceiverExt<T> for tokio::sync::watch::Receiver<T> {
    async fn wait_for_change(
        &mut self,
        f: impl Fn(&T) -> bool,
    ) -> Result<(), RecvError>{
        loop {
            self.changed().await?;
            if f(&self.borrow()) {
                return Ok(());
            }
        }
    }
}
