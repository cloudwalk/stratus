//! Metrics configuration.

use std::stringify;

use metrics::counter;
use metrics::describe_counter;
use metrics::describe_histogram;
use metrics::histogram;
use metrics::Label as MetricsLabel;
use metrics_exporter_prometheus::PrometheusBuilder;
use paste::paste;

use crate::metrics;
use crate::metrics_impl_describe;
use crate::metrics_impl_fn_inc;

/// Init application metrics.
pub fn init_metrics() {
    // default configuration runs metrics exporter on port 9000
    PrometheusBuilder::new().install().expect("Metrics initialization failed");
    register_metrics();

    tracing::info!("metrics initialized");
}

// Create all applications metrics.
metrics! {
    "Ethereum JSON-RPC requests that started."
    counter   rpc_requests_started{method, function},

    "Ethereum JSON-RPC requests that finished."
    histogram rpc_requests_finished{method, function, success}
}

// -----------------------------------------------------------------------------
// Labels
// -----------------------------------------------------------------------------

/// Representation of a label value.
///
/// It exists to improve two aspects `metrics` crate does not cover:
/// * Conversion from several types to a label value.
/// * Handling of optional values.
pub enum LabelValue {
    /// Label has a value and should be recorded.
    Some(String),
    /// Label does not have a value and should be ignored.
    None,
}

impl From<&str> for LabelValue {
    fn from(value: &str) -> Self {
        Self::Some(value.to_owned())
    }
}

impl From<Option<&str>> for LabelValue {
    fn from(value: Option<&str>) -> Self {
        match value {
            Some(value) => Self::Some(value.to_owned()),
            None => Self::None,
        }
    }
}

impl From<String> for LabelValue {
    fn from(value: String) -> Self {
        Self::Some(value)
    }
}

impl From<bool> for LabelValue {
    fn from(value: bool) -> Self {
        Self::Some(value.to_string())
    }
}

/// Converts a list of label keys-value pairs to `metrics::Label`. Labels with missing values are filtered out.
fn into_labels(labels: Vec<(&'static str, LabelValue)>) -> Vec<MetricsLabel> {
    labels
        .into_iter()
        .filter_map(|(key, value)| match value {
            LabelValue::Some(value) => Some((key, value)),
            LabelValue::None => None,
        })
        .map(|(key, value)| MetricsLabel::new(key, value))
        .collect()
}

// -----------------------------------------------------------------------------
// Macros
// -----------------------------------------------------------------------------

/// Generate functions to record metrics.
#[macro_export]
macro_rules! metrics {
    (
        $(
            $description:literal
            $kind:ident $name:ident{ $($label:ident),+ }
        ),+
    ) => {
        // Register metrics with description with the provider
        fn register_metrics() {
            $(
                metrics_impl_describe!($kind $name $description);
            )+
        }

        // Record metrics
        $(
            metrics_impl_fn_inc!($kind $name $($label)+);
        )+
    }
}

/// Internal - Generates a statement that describe a metrics.
#[macro_export]
macro_rules! metrics_impl_describe {
    (counter $name:ident $description:literal) => {
        paste! {
            describe_counter!(stringify!([<stratus_$name>]),  $description)
        }
    };
    (histogram  $name:ident $description:literal) => {
        paste! {
            describe_histogram!(stringify!([<stratus_$name>]), $description)
        }
    };
}

/// Internal - Generates a function that increases a metric value.
#[macro_export]
macro_rules! metrics_impl_fn_inc {
    (counter $name:ident $($label:ident)+) => {
        paste! {
            #[doc = "Increment 1 to the `" $name "` counter."]
            pub fn [<inc_ $name>]($( $label: impl Into<LabelValue> ),+) {
                let labels = into_labels(
                    vec![
                        $(
                            (stringify!($label), $label.into()),
                        )+
                    ]
                );
                counter!(stringify!([<stratus_$name>]), 1, labels);
            }
        }
    };
    (histogram  $name:ident $($label:ident)+) => {
        paste! {
            #[doc = "Increase the duration of the `" $name "` histogram."]
            pub fn [<inc_ $name>](duration: std::time::Duration, $( $label: impl Into<LabelValue> ),+) {
                let labels = into_labels(
                    vec![
                        $(
                            (stringify!($label), $label.into()),
                        )+
                    ]
                );
                histogram!(stringify!([<stratus_$name>]), duration, labels)
            }
        }
    };
}
