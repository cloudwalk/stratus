//! Standard library extensions.

use std::time::Duration;

use anyhow::anyhow;
use tokio::select;
use tokio::signal::unix::signal;
use tokio::signal::unix::SignalKind;
use tracing::Span;

use crate::infra::tracing::info_task_spawn;
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

/// Gets the current binary basename.
pub fn binary_name() -> String {
    let binary = std::env::current_exe().unwrap();
    let binary_basename = binary.file_name().unwrap().to_str().unwrap().to_lowercase();

    if binary_basename.starts_with("test_") {
        "tests".to_string()
    } else {
        binary_basename
    }
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
        self.expect("serialization should be infallible")
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

/// Reads a value from a channel logging timeout at some predefined interval.
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

// -----------------------------------------------------------------------------
// Tracing
// -----------------------------------------------------------------------------

/// Extensions for `tracing::Span`.
pub trait SpanExt {
    /// Applies the provided function to the current span.
    fn with<F>(fill: F)
    where
        F: Fn(Span),
    {
        if cfg!(tracing) {
            let span = Span::current();
            fill(span);
        }
    }

    /// Records a value using `ToString` implementation.
    fn rec_str<T>(&self, field: &'static str, value: &T)
    where
        T: ToString;

    /// Records a value using `ToString` implementation if the option value is present.
    fn rec_opt<T>(&self, field: &'static str, value: &Option<T>)
    where
        T: ToString;
}

impl SpanExt for Span {
    fn rec_str<T>(&self, field: &'static str, value: &T)
    where
        T: ToString,
    {
        self.record(field, value.to_string().as_str());
    }

    fn rec_opt<T>(&self, field: &'static str, value: &Option<T>)
    where
        T: ToString,
    {
        if let Some(ref value) = value {
            self.record(field, value.to_string().as_str());
        }
    }
}

/// Logs an error and also wrap the existing error with the provided message.
#[macro_export]
macro_rules! log_and_err {
    // with reason: wrap the original error with provided message
    (reason = $error:ident, payload = $payload:expr, $msg:expr) => {
        {
            use anyhow::Context;
            tracing::error!(reason = ?$error, payload = ?$payload, message = %$msg);
            Err($error).context($msg)
        }
    };
    (reason = $error:ident, $msg:expr) => {
        {
            use anyhow::Context;
            tracing::error!(reason = ?$error, message = %$msg);
            Err($error).context($msg)
        }
    };
    // without reason: generate a new error using provided message
    (payload = $payload:expr, $msg:expr) => {
        {
            use anyhow::Context;
            use anyhow::anyhow;
            tracing::error!(payload = ?$payload, message = %$msg);
            let message = format!("{} | payload={:?}", $msg, $payload);
            Err(anyhow!(message))
        }
    };
    ($msg:expr) => {
        {
            use anyhow::anyhow;
            tracing::error!(message = %$msg);
            Err(anyhow!($msg))
        }
    };
}

// -----------------------------------------------------------------------------
// Tokio
// -----------------------------------------------------------------------------

/// Spawns an async Tokio task with a name to be displayed in tokio-console.
#[track_caller]
pub fn named_spawn<T>(name: &str, task: impl std::future::Future<Output = T> + Send + 'static) -> tokio::task::JoinHandle<T>
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
pub fn named_spawn_blocking<T>(name: &str, task: impl FnOnce() -> T + Send + 'static) -> tokio::task::JoinHandle<T>
where
    T: Send + 'static,
{
    info_task_spawn(name);
    tokio::task::Builder::new()
        .name(name)
        .spawn_blocking(task)
        .expect("spawning named blocking task should not fail")
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

    named_spawn("sys::signal_handler", async move {
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
