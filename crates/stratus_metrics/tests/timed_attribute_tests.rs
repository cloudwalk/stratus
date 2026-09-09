//! Runtime tests for the `#[timed]` attribute macro.
//!
//! Integration tests compile as separate crates that depend on
//! `stratus_metrics`, so the macro expansion resolves `::stratus_metrics::`
//! paths and the `feature = "metrics"` cfg the same way a real consumer does.
//!
//! The attribute's recording path (and with it, label derivation) only exists
//! with the `metrics` feature enabled, so these tests require it; with
//! `--no-default-features` the attribute expands to the plain function body
//! and this file is compiled out.

#![cfg(feature = "metrics")]

use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use stratus_metrics::MetricLabelValue;
use stratus_metrics::ToMetricLabelValue;
use stratus_metrics::timed;

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

#[tokio::test]
async fn supports_async_functions() {
    measured_async().await;
}
