//! Metrics configuration.

use std::stringify;

use metrics::counter;
use metrics::describe_counter;
use metrics::describe_histogram;
use metrics::histogram;
use metrics::IntoLabels;
use metrics_exporter_prometheus::PrometheusBuilder;
use paste::paste;

use crate::metrics;
use crate::metrics_impl_describe;
use crate::metrics_impl_fn_record;

/// Init application metrics.
pub fn init_metrics() {
    PrometheusBuilder::new().install().expect("Metrics initialization failed"); // http listener on default port 9000

    register_metrics();

    tracing::info!("metrics initialized");
}

// Describe all applications metrics.
metrics! {
    counter    eth_rpc_request_started   "Ethereum JSON-RPC requests that started.",
    histogram  eth_rpc_request_finished  "Ethereum JSON-RPC requests that finished."
}

/// Generate functions to record metrics.
#[macro_export]
macro_rules! metrics {
    ($($kind:ident $name:ident $description:literal),+) => {
        // Register metrics with description with the provider
        fn register_metrics() {
            $(
                metrics_impl_describe!($kind $name $description);
            )+
        }

        // Record metrics
        $(
            metrics_impl_fn_record!($kind $name);
        )+
    }
}

/// Internal - Generates a statement that describe a metrics.
#[macro_export]
macro_rules! metrics_impl_describe {
    (counter $name:ident $description:literal) => {
        paste! {
            describe_counter!(stringify!($name),  $description)
        }
    };
    (histogram  $name:ident $description:literal) => {
        paste! {
            describe_histogram!(stringify!($name),  $description)
        }
    };
}

/// Internal - Generates a function that records a new metric value.
#[macro_export]
macro_rules! metrics_impl_fn_record {
    (counter $name:ident) => {
        paste! {
            #[doc = "Increment 1 to the `" $name "` counter."]
            pub fn [<inc_ $name>](labels: impl IntoLabels) {
                counter!(stringify!($name), 1, labels.into_labels());
            }
        }
    };
    (histogram  $name:ident) => {
        paste! {
            #[doc = "Increase the duration of the `" $name "` histogram."]
            pub fn [<inc_ $name>](duration: std::time::Duration, labels: impl IntoLabels) {
                histogram!(stringify!($name), duration, labels.into_labels())
            }
        }
    };
}
