//! Metrics services.

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

/// Buckets in seconds.
const BUCKETS: [f64; 29] = [
    //
    0.0005, 0.001, 0.002, 0.003, 0.004, 0.005, 0.006, 0.007, 0.008, 0.009, 0.01, 0.02, 0.03, 0.04, 0.05, 0.06, 0.07, 0.08, 0.09, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6,
    0.7, 0.8, 0.9, 1.0,
];

/// Init application global metrics.
///
/// Default configuration runs metrics exporter on port 9000.
pub fn init_metrics() {
    tracing::info!("starting metrics");

    PrometheusBuilder::new()
        .set_buckets(&BUCKETS)
        .unwrap()
        .install()
        .expect("failed to start metrics");

    // api metrics
    register_metrics_for_json_rpc();

    // executor metrics
    register_metrics_for_executor();
    register_metrics_for_evm();

    // storage metrics
    register_metrics_for_storage_read();
    register_metrics_for_storage_write();
}

// JSON-RPC metrics.
metrics! {
    group: json_rpc,

    "Number of JSON-RPC requests that started."
    counter   rpc_requests_started{method, function},

    "Number of JSON-RPC requests that finished."
    histogram rpc_requests_finished{method, function, success}
}

// Storage reads.
metrics! {
    group: storage_read,

    "Time to execute storage read_current_block_number operation."
    histogram storage_read_current_block_number{success},

    "Time to execute storage read_account operation."
    histogram storage_read_account{kind, point_in_time, success},

    "Time to execute storage read_block operation."
    histogram storage_read_block{success},

    "Time to execute storage read_logs operation."
    histogram storage_read_logs{success},

    "Time to execute storage read_slot operation."
    histogram storage_read_slot{kind, point_in_time, success},

    "Time to execute storage read_mined_transaction operation."
    histogram storage_read_mined_transaction{success}
}

// Storage writes.
metrics! {
    group: storage_write,

    "Time to execute storage increment_block_number operation."
    histogram storage_increment_block_number{success},

    "Time to execute storage set_block_number operation."
    histogram storage_set_block_number{success},

    "Time to execute storage save_accounts operation."
    histogram storage_save_accounts{success},

    "Time to execute storage save_account_changes operation."
    histogram storage_save_account_changes{success},

    "Time to execute storage save_block operation."
    histogram storage_save_block{success},

    "Time to execute storage reset operation."
    histogram storage_reset{kind, success},

    "Time to execute storage commit operation."
    histogram storage_commit{success}
}

// Execution metrics.
metrics! {
    group: executor,

    "Time to execute import_offline operation."
    histogram executor_import_offline{},

    "Time to execute and persist temporary changes of a single transaction inside import_offline operation."
    histogram executor_import_offline_transaction{},

    "Time to execute import_online operation."
    histogram executor_import_online{},

    "Time to execute and persist a single transaction inside import_online operation."
    histogram executor_import_online_transaction{},

    "Time to execute a transaction received with eth_sendRawTransaction."
    histogram executor_transact{success},

    "Time to execute a transaction received with eth_call or eth_estimateGas."
    histogram executor_call{success}
}

// EVM metrics.
metrics! {
    group: evm,

    "Time to execute EVM execution."
    histogram evm_execution{point_in_time, success}
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

/// Internal - Generate functions to record metrics.
#[macro_export]
#[doc(hidden)]
macro_rules! metrics {
    (
        group: $group:ident,
        $(
            $description:literal
            $kind:ident $name:ident{ $($label:ident),* }
        ),+
    ) => {
        // Register metrics with description with the provider
        paste! {
            fn [<register_metrics_for_ $group>]() {
                $(
                    metrics_impl_describe!($kind $name $description);
                )+
            }
        }

        // Record metrics
        $(
            metrics_impl_fn_inc!($kind $name $group $($label)*);
        )+
    }
}

/// Internal - Generates a statement that describe a metrics.
#[macro_export]
#[doc(hidden)]
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
#[doc(hidden)]
macro_rules! metrics_impl_fn_inc {
    (counter $name:ident $group:ident $($label:ident)*) => {
        paste! {
            #[doc = "Add 1 to `" $name "` counter."]
            pub fn [<inc_ $name>]($( $label: impl Into<LabelValue> ),+) {
                let labels = into_labels(
                    vec![
                        ("group", stringify!($group).into()),
                        $(
                            (stringify!($label), $label.into()),
                        )+
                    ]
                );
                counter!(stringify!([<stratus_$name>]), 1, labels);
            }
        }
    };
    (histogram  $name:ident $group:ident $($label:ident)*) => {
        paste! {
            #[doc = "Add operation duration to `" $name "` histogram."]
            pub fn [<inc_ $name>](duration: std::time::Duration, $( $label: impl Into<LabelValue> ),*) {
                let labels = into_labels(
                    vec![
                        ("group", stringify!($group).into()),
                        $(
                            (stringify!($label), $label.into()),
                        )*
                    ]
                );
                histogram!(stringify!([<stratus_$name>]), duration, labels)
            }
        }
    };
}
