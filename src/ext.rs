//! Standard library extensions.

// -----------------------------------------------------------------------------
// Macros
// -----------------------------------------------------------------------------

use std::time::Duration;

use tokio::runtime::Builder;
use tokio::runtime::Runtime;

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

// -----------------------------------------------------------------------------
// Tokio
// -----------------------------------------------------------------------------

/// Creates a new Tokio runtime with the specified configuration.
pub fn new_tokio_runtime(thread_name: &'static str, num_async_threads: usize, num_blocking_threads: usize) -> Runtime {
    Builder::new_multi_thread()
        .enable_all()
        .thread_name(thread_name)
        .worker_threads(num_async_threads)
        .max_blocking_threads(num_blocking_threads)
        .thread_keep_alive(Duration::from_secs(u64::MAX))
        .build()
        .expect("failed to build tokio runtime")
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
// Standalone functions
// -----------------------------------------------------------------------------

/// `not(something)` instead of `!something`.
#[inline(always)]
pub fn not(value: bool) -> bool {
    !value
}
