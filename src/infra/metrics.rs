//! Metrics services.
#![cfg(feature = "metrics")]

use std::stringify;
use std::time::Instant;

use metrics::counter;
use metrics::describe_counter;
use metrics::describe_gauge;
use metrics::describe_histogram;
use metrics::gauge;
use metrics::histogram;
use metrics::Label as MetricsLabel;
use metrics_exporter_prometheus::Matcher;
use metrics_exporter_prometheus::PrometheusBuilder;
use paste::paste;

use crate::config::MetricsHistogramKind;
use crate::ext::not;
use crate::metrics;
use crate::metrics_impl_fn_inc;

/// Default bucket for duratino based metrics.
const BUCKET_FOR_DURATION: [f64; 37] = [
    0.0001, 0.0002, 0.0003, 0.0004, 0.0005, 0.0006, 0.0007, 0.0008, 0.0009, // 0.1ms to 0.9ms
    0.001, 0.002, 0.003, 0.004, 0.005, 0.006, 0.007, 0.008, 0.009, // 1ms to 9ms
    0.01, 0.02, 0.03, 0.04, 0.05, 0.06, 0.07, 0.08, 0.09, // 10ms to 90ms
    0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, // 100ms to 900ms
    1.,  // 1s or more
];

/// Init application global metrics.
///
/// Default configuration runs metrics exporter on port 9000.
pub fn init_metrics(histogram_kind: MetricsHistogramKind) {
    tracing::info!("starting metrics");

    // get metric definitions
    let mut metrics = Vec::new();
    metrics.extend(metrics_for_importer_offline());
    metrics.extend(metrics_for_importer_online());
    metrics.extend(metrics_for_json_rpc());
    metrics.extend(metrics_for_executor());
    metrics.extend(metrics_for_evm());
    metrics.extend(metrics_for_storage_read());
    metrics.extend(metrics_for_storage_write());
    metrics.extend(metrics_for_rocks());

    // init exporter
    let mut builder = PrometheusBuilder::new();

    // init buckets (comment to use summary)
    if histogram_kind == MetricsHistogramKind::Histogram {
        builder = builder.set_buckets(&BUCKET_FOR_DURATION).unwrap();
        for metric in &metrics {
            if metric.has_custom_buckets() {
                builder = builder.set_buckets_for_metric(Matcher::Full(metric.name.to_string()), &metric.buckets).unwrap();
            }
        }
    }

    builder.install().expect("failed to start metrics");

    // init metric description (always after provider started)
    for metric in &metrics {
        metric.register_description();
    }
}

/// Track metrics execution starting instant.
pub fn now() -> Instant {
    Instant::now()
}

// JSON-RPC metrics.
metrics! {
    group: json_rpc,

    "Number of JSON-RPC requests that started."
    counter   rpc_requests_started{method, function} [],

    "Number of JSON-RPC requests that finished."
    histogram_duration rpc_requests_finished{method, function, success} []
}

// Storage reads.
metrics! {
    group: storage_read,

    "Time to execute storage read_active_block_number operation."
    histogram_duration storage_read_active_block_number{success} [],

    "Time to execute storage read_mined_block_number operation."
    histogram_duration storage_read_mined_block_number{success} [],

    "Time to execute storage read_account operation."
    histogram_duration storage_read_account{found_at, point_in_time, success} [],

    "Time to execute storage read_block operation."
    histogram_duration storage_read_block{success} [],

    "Time to execute storage read_logs operation."
    histogram_duration storage_read_logs{success} [],

    "Time to execute storage read_slot operation."
    histogram_duration storage_read_slot{found_at, point_in_time, success} [],

    "Time to execute storage read_slot operation."
    histogram_duration storage_read_slots{point_in_time, success} [],

    "Time to execute storage read_mined_transaction operation."
    histogram_duration storage_read_mined_transaction{success} []
}

// Storage writes.
metrics! {
    group: storage_write,

    "Time to execute storage set_active_block_number operation."
    histogram_duration storage_set_active_block_number{success} [],

    "Time to execute storage set_mined_block_number operation."
    histogram_duration storage_set_mined_block_number{success} [],

    "Time to execute storage save_accounts operation."
    histogram_duration storage_save_accounts{success} [],

    "Time to execute storage save_account_changes operation."
    histogram_duration storage_save_execution{success} [],

    "Time to execute storage flush_temp operation."
    histogram_duration storage_flush_temp{success} [],

    "Time to execute storage save_block operation."
    histogram_duration storage_save_block{success} [],

    "Time to execute storage reset operation."
    histogram_duration storage_reset{kind, success} [],

    "Time to execute storage commit operation."
    histogram_duration storage_commit{size_by_tx, size_by_gas, success} [],

    "Ammount of gas in the commited transactions"
    counter   storage_gas_total{} []
}

// Importer offline metrics.
metrics! {
    group: importer_offline,

    "Time to execute import_offline operation."
    histogram_duration import_offline{} []
}

// Importer online metrics.
metrics! {
    group: importer_online,

    "Time to execute import_online operation."
    histogram_duration import_online{} [],

    "Time to import one mined block."
    histogram_duration import_online_mined_block{} [],

    "Transactions imported"
    counter importer_online_transactions_total{} []
}

// Execution metrics.
metrics! {
    group: executor,

    "Time to execute and persist an external block with all transactions."
    histogram_duration executor_external_block{} [],

    "Time to execute and persist temporary changes of a single transaction inside import_offline operation."
    histogram_duration executor_external_transaction{} [],

    "Number of account reads when importing an external block."
    histogram_counter executor_external_block_account_reads{} [0., 1., 2., 3., 4., 5., 6., 7., 8., 9., 10., 20., 30., 40., 50., 60., 70., 80., 90., 100., 150., 200.],

    "Number of slot reads when importing an external block."
    histogram_counter executor_external_block_slot_reads{} [0., 10., 20., 30., 40., 50., 60., 70., 80., 90., 100., 200., 300., 400., 500., 600., 700., 800., 900., 1000., 2000., 3000., 4000., 5000., 6000., 7000., 8000., 9000., 10000.],

    "Number of slot reads cached when importing an external block."
    histogram_counter executor_external_block_slot_reads_cached{} [0., 10., 20., 30., 40., 50., 60., 70., 80., 90., 100., 200., 300., 400., 500., 600., 700., 800., 900., 1000., 2000., 3000., 4000., 5000., 6000., 7000., 8000., 9000., 10000.],

    "Time to execute a transaction received with eth_sendRawTransaction."
    histogram_duration executor_transact{success} [],

    "Time to execute a transaction received with eth_call or eth_estimateGas."
    histogram_duration executor_call{success} []
}

metrics! {
    group: evm,

    "Time to execute EVM execution."
    histogram_duration evm_execution{point_in_time, success} [],

    "Number of accounts read in a single EVM execution."
    histogram_counter evm_execution_account_reads{} [0., 1., 2., 3., 4., 5., 6., 7., 8., 9., 10.],

    "Number of slots read in a single EVM execution."
    histogram_counter evm_execution_slot_reads{} [0., 10., 20., 30., 40., 50., 60., 70., 80., 90., 100., 200., 300., 400., 500., 600., 700., 800., 900., 1000.],

    "Number of slots read cached in a single EVM execution."
    histogram_counter evm_execution_slot_reads_cached{} [0., 10., 20., 30., 40., 50., 60., 70., 80., 90., 100., 200., 300., 400., 500., 600., 700., 800., 900., 1000.]
}

metrics! {
    group: rocks,

    "Number of issued gets to rocksdb."
    gauge rocks_db_get{dbname} [],

    "Number of writes issued to rocksdb."
    gauge rocks_db_write{dbname} [],

    "Time spent compacting data."
    gauge rocks_compaction_time{dbname} [],

    "CPU time spent compacting data."
    gauge rocks_compaction_cpu_time{dbname} [],

    "Time spent flushing memtable to disk."
    gauge rocks_flush_time{dbname} [],

    "Number of block cache misses."
    gauge rocks_block_cache_miss{dbname} [],

    "Number of block cache hits."
    gauge rocks_block_cache_hit{dbname} [],

    "Number of bytes written."
    gauge rocks_bytes_written{dbname} [],

    "Number of bytes read."
    gauge rocks_bytes_read{dbname} []
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
            $buckets:expr
        ),+
    ) => {
        // Generate function to get metric definition.
        paste! {
            // Generate constant to access by name.
            $(
                pub const [<METRIC_ $name:upper>]: &str = stringify!([<stratus_ $name>]);
            )+

            // Generate function that return metric definition.
            fn [<metrics_for_ $group>]() -> Vec<Metric> {
                vec![
                    $(
                        Metric {
                            kind: stringify!($kind),
                            name: stringify!([<stratus_ $name>]),
                            description: stringify!($description),
                            buckets: $buckets.to_vec()
                        },
                    )+
                ]
            }
        }

        // Generate function to record metrics values.
        $(
            metrics_impl_fn_inc!($kind $name $group $($label)*);
        )+
    }
}

/// Internal - Generates a function that increases a metric value.
#[macro_export]
#[doc(hidden)]
macro_rules! metrics_impl_fn_inc {
    (counter $name:ident $group:ident $($label:ident)*) => {
        paste! {
            #[doc = "Add n to `" $name "` counter."]
            pub fn [<inc_n_ $name>](n: u64, $( $label: impl Into<LabelValue> ),*) {
                let labels = into_labels(
                    vec![
                        ("group", stringify!($group).into()),
                        $(
                            (stringify!($label), $label.into()),
                        )*
                    ]
                );
                counter!(stringify!([<stratus_$name>]), n, labels);
            }
        }

        paste! {
            #[doc = "Add 1 to `" $name "` counter."]
            pub fn [<inc_ $name>]($( $label: impl Into<LabelValue> ),*) {
                let labels = into_labels(
                    vec![
                        ("group", stringify!($group).into()),
                        $(
                            (stringify!($label), $label.into()),
                        )*
                    ]
                );
                counter!(stringify!([<stratus_$name>]), 1, labels);
            }
        }
    };
    (histogram_counter  $name:ident $group:ident $($label:ident)*) => {
        paste! {
            #[doc = "Add N to `" $name "` histogram."]
            pub fn [<inc_ $name>](n: usize, $( $label: impl Into<LabelValue> ),*) {
                let labels = into_labels(
                    vec![
                        ("group", stringify!($group).into()),
                        $(
                            (stringify!($label), $label.into()),
                        )*
                    ]
                );
                histogram!(stringify!([<stratus_$name>]), n as f64, labels);
            }
        }
    };
    (histogram_duration  $name:ident $group:ident $($label:ident)*) => {
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
                histogram!(stringify!([<stratus_$name>]), duration, labels);
            }
        }
    };
    (gauge  $name:ident $group:ident $($label:ident)*) => {
        paste! {
            #[doc = "Set `" $name "` gauge."]
            pub fn [<set_ $name>](n: u64, $( $label: impl Into<LabelValue> ),*) {
                let labels = into_labels(
                    vec![
                        ("group", stringify!($group).into()),
                        $(
                            (stringify!($label), $label.into()),
                        )*
                    ]
                );
                gauge!(stringify!([<stratus_$name>]), n as f64, labels);
            }
        }
    };
}

/// Metric defintion.
struct Metric {
    kind: &'static str,
    name: &'static str,
    description: &'static str,
    buckets: Vec<f64>,
}

impl Metric {
    /// Checks if metric has custom buckets defined.
    fn has_custom_buckets(&self) -> bool {
        not(self.buckets.is_empty())
    }

    /// Register description with the provider.
    fn register_description(&self) {
        match self.kind {
            "counter" => describe_counter!(self.name, self.description),
            "histogram_duration" | "histogram_counter" => describe_histogram!(self.name, self.description),
            "gauge" => describe_gauge!(self.name, self.description),
            _ => {}
        }
    }
}
