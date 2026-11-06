//! Runtime tests for the `#[timed]` attribute macro.
//!
//! Integration tests compile as separate crates that depend on
//! `stratus_metrics`, so the macro expansion resolves `::stratus_metrics::`
//! paths and the `feature = "metrics"` cfg the same way a real consumer does.
//!
//! The generated recording statements (and with them, label derivation) only
//! exist with the `metrics` feature enabled, so these tests require it. The
//! original function body is emitted once regardless of the feature, and this
//! test file is compiled out with `--no-default-features`.

#![cfg(feature = "metrics")]

use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use stratus_metrics::MetricLabelValue;
use stratus_metrics::ToMetricLabelValue;
use stratus_metrics::timed;

static INPUT_LABEL_CALLS: AtomicUsize = AtomicUsize::new(0);
static RESULT_LABEL_CALLS: AtomicUsize = AtomicUsize::new(0);
static MARKER_DROP_ORDER: AtomicUsize = AtomicUsize::new(0);
static DURATION_OVERRIDE_CALLS: AtomicUsize = AtomicUsize::new(0);
static DURATION_RESULT_LABEL_CALLS: AtomicUsize = AtomicUsize::new(0);

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

#[timed(storage_finish_pending_block)]
async fn measured_async_from_marker() {
    let _setup = ();
    stratus_metrics::timed_start!();
    tokio::task::yield_now().await;
    stratus_metrics::timed_end!();
    tokio::task::yield_now().await;
}

#[timed(storage_finish_pending_block)]
fn measured_until_end_marker() {
    stratus_metrics::timed_end!();
}

#[timed(
    storage_read_block,
    labels(
        storage = "custom",
        success = {
            DURATION_RESULT_LABEL_CALLS.fetch_add(1, Ordering::Relaxed);
            result.is_ok()
        },
    )
)]
fn measured_duration_override(return_early: bool) -> Result<(), ()> {
    if return_early {
        return Err(());
    }

    stratus_metrics::timed_duration!({
        DURATION_OVERRIDE_CALLS.fetch_add(1, Ordering::Relaxed);
        std::time::Duration::from_millis(5)
    });
    Ok(())
}

struct MarkerGuard;

impl Drop for MarkerGuard {
    fn drop(&mut self) {
        assert_eq!(MARKER_DROP_ORDER.fetch_add(1, Ordering::Relaxed), 0);
    }
}

struct MarkerResult;

impl Drop for MarkerResult {
    fn drop(&mut self) {
        assert_eq!(MARKER_DROP_ORDER.fetch_add(1, Ordering::Relaxed), 1);
    }
}

#[timed(storage_finish_pending_block)]
fn measured_from_marker() -> MarkerResult {
    let _guard = MarkerGuard;
    stratus_metrics::timed_start!();
    MarkerResult
}

#[test]
fn record_publishes_and_returns_the_operation_result() {
    let result = stratus_metrics::record(|| "result".to_owned(), |_, result| assert_eq!(result, "result"));
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

#[test]
fn custom_duration_falls_back_when_its_marker_is_not_reached() {
    DURATION_OVERRIDE_CALLS.store(0, Ordering::Relaxed);
    DURATION_RESULT_LABEL_CALLS.store(0, Ordering::Relaxed);

    assert!(measured_duration_override(false).is_ok());
    assert!(measured_duration_override(true).is_err());
    assert_eq!(DURATION_OVERRIDE_CALLS.load(Ordering::Relaxed), 1);
    assert_eq!(DURATION_RESULT_LABEL_CALLS.load(Ordering::Relaxed), 2);
}

#[test]
fn marker_drops_setup_values_before_the_returned_value() {
    MARKER_DROP_ORDER.store(0, Ordering::Relaxed);

    let result = measured_from_marker();
    assert_eq!(MARKER_DROP_ORDER.load(Ordering::Relaxed), 1);
    drop(result);
    assert_eq!(MARKER_DROP_ORDER.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn supports_async_functions() {
    measured_async().await;
    measured_async_from_marker().await;
    measured_until_end_marker();
}
