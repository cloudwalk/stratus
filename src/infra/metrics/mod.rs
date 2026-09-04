mod metrics_config;
mod metrics_definitions;
mod metrics_macros;
mod metrics_types;

use std::future::Future;
use std::time::Duration;
use std::time::Instant;

pub use metrics_config::MetricsConfig;
pub use metrics_definitions::*;
pub use metrics_types::*;

use crate::eth::executor::EvmKind;

/// Track metrics execution starting instant.
pub fn now() -> Instant {
    Instant::now()
}

/// Executes an operation, publishes its elapsed time and result, and returns
/// the result unchanged.
pub fn record<T>(operation: impl FnOnce() -> T, publish: impl FnOnce(Duration, &T)) -> T {
    let start = now();
    let result = operation();
    publish(start.elapsed(), &result);
    result
}

/// Async variant of [`record`].
pub async fn record_async<T, F>(operation: impl FnOnce() -> F, publish: impl FnOnce(Duration, &T)) -> T
where
    F: Future<Output = T>,
{
    let start = now();
    let result = operation().await;
    publish(start.elapsed(), &result);
    result
}

// -----------------------------------------------------------------------------
// Executor pool busy workers gauge
// -----------------------------------------------------------------------------

impl EvmKind {
    /// Marks a worker in the given executor pool as busy by atomically incrementing the `executor_workers_busy` gauge.
    /// Returns a guard that atomically decrements the gauge when dropped.
    pub fn mark_executor_pool_busy(&self) -> BusyGuard {
        inc_executor_workers_busy(1, *self);
        BusyGuard(*self)
    }

    /// Marks a worker in the given executor pool as free by atomically decrementing the `executor_workers_busy` gauge.
    fn mark_executor_pool_free(&self) {
        dec_executor_workers_busy(1, *self);
    }
}

pub struct BusyGuard(EvmKind);

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.mark_executor_pool_free();
    }
}

#[cfg(test)]
mod attribute_tests {
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use stratus_macros::timed;

    use super::MetricLabelValue;
    use super::ToMetricLabelValue;

    static INPUT_LABEL_CALLS: AtomicUsize = AtomicUsize::new(0);
    static RESULT_LABEL_CALLS: AtomicUsize = AtomicUsize::new(0);

    struct NonCloneLabel(&'static str);

    impl ToMetricLabelValue for NonCloneLabel {
        fn to_metric_label_value(&self) -> MetricLabelValue {
            self.0.into()
        }
    }

    #[timed(
        storage_read_block,
        labels(
            storage = |storage| {
                INPUT_LABEL_CALLS.fetch_add(1, Ordering::Relaxed);
                storage.as_str()
            },
            success = {
                RESULT_LABEL_CALLS.fetch_add(1, Ordering::Relaxed);
                result.is_ok()
            },
        )
    )]
    fn measured_result(storage: String, fail: bool) -> Result<(), ()> {
        drop(storage);
        if fail {
            Err(())?;
        }
        Ok(())
    }

    fn consume_non_clone_label(_: NonCloneLabel) {}

    #[timed(executor_inspect, labels(trace_type))]
    fn measured_parameter(trace_type: NonCloneLabel) {
        consume_non_clone_label(trace_type);
    }

    #[timed(storage_finish_pending_block)]
    async fn measured_async() {
        tokio::task::yield_now().await;
    }

    #[test]
    fn record_publishes_and_returns_the_operation_result() {
        let result = super::record(|| "result".to_owned(), |_, result| assert_eq!(result, "result"));
        assert_eq!(result, "result");
    }

    #[test]
    fn derives_labels_before_and_after_early_return() {
        INPUT_LABEL_CALLS.store(0, Ordering::Relaxed);
        RESULT_LABEL_CALLS.store(0, Ordering::Relaxed);

        assert!(measured_result("memory".to_owned(), false).is_ok());
        assert!(measured_result("memory".to_owned(), true).is_err());
        assert_eq!(INPUT_LABEL_CALLS.load(Ordering::Relaxed), 2);
        assert_eq!(RESULT_LABEL_CALLS.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn converts_non_clone_parameter_labels_before_the_body_consumes_them() {
        measured_parameter(NonCloneLabel("call_tracer"));
    }

    #[tokio::test]
    async fn supports_async_functions() {
        measured_async().await;
    }
}
