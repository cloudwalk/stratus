//! Standard library extensions.

// -----------------------------------------------------------------------------
// Macros
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

/// Generates unit test that checks implementation of [`Serialize`](serde::Serialize) and [`Deserialize`](serde::Deserialize) are compatible.
#[macro_export]
macro_rules! gen_test_serde {
    ($type:ty) => {
        paste::paste! {
            #[test]
            pub fn [<serde_ $type:snake>]() {
                let value = <fake::Faker as fake::Fake>::fake::<$type>(&fake::Faker);
                let json = serde_json::to_string(&value).unwrap();
                assert_eq!(serde_json::from_str::<$type>(&json).unwrap(), value);
            }
        }
    };
}

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
// Tracing
// -----------------------------------------------------------------------------

/// Emits an warning that a task is exiting because it received a cancenllation signal.
#[track_caller]
pub fn warn_task_cancellation(task: &str) {
    let message = format!("exiting {} because it received a cancellation signal", task);
    tracing::warn!(%message);
}

/// Emits an warning that a task is exiting because the tx side was closed.
#[track_caller]
pub fn warn_task_tx_closed(task: &str) {
    let message = format!("exiting {} because the tx channel on the other side was closed", task);
    tracing::warn!(%message);
}

/// Emits an warning that a task is exiting because the rx side was closed.
#[track_caller]
pub fn warn_task_rx_closed(task: &str) {
    let message = format!("exiting {} because the rx channel on the other side was closed", task);
    tracing::warn!(%message);
}

// -----------------------------------------------------------------------------
// Tokio
// -----------------------------------------------------------------------------

/// Spawns a Tokio task with a name to be displayed in tokio-console.
#[track_caller]
pub fn spawn_named<T>(name: &str, task: impl std::future::Future<Output = T> + Send + 'static) -> tokio::task::JoinHandle<T>
where
    T: Send + 'static,
{
    tokio::task::Builder::new().name(name).spawn(task).expect("spawning named task should not fail")
}

// -----------------------------------------------------------------------------
// Standalone functions
// -----------------------------------------------------------------------------

/// `not(something)` instead of `!something`.
#[inline(always)]
pub fn not(value: bool) -> bool {
    !value
}
