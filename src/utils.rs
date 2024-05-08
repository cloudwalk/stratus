use std::future::Future;

use tracing::Instrument;

use crate::infra::tracing::LogFormatter;

/// Runs future in an unique tracing context ID.
///
/// # Output Example
///
/// Tracing logs emmited in this future will start with a context ID:
///
/// ```txt
///                                  vvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvv
/// 2024-05-08T15:35:51.186943Z INFO [42b7bdb5-b8fa-4b85-9788-a868bf6d8e10] span1: field1=val1 ...
/// ```
pub async fn with_unique_logging_context<T>(fut: impl Future<Output = T>) -> T {
    let span = LogFormatter::new_context_id_span();
    fut.instrument(span).await
}

/// Runs closure in an unique tracing context ID.
///
/// Sync version of [`in_unique_logging_context`].
pub fn with_unique_logging_context_sync<T>(f: impl Fn() -> T) -> T {
    let span = LogFormatter::new_context_id_span();
    span.in_scope(f)
}
