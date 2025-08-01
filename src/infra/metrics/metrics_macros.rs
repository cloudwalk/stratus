use std::sync::OnceLock;

use crate::infra::metrics::metrics_collector::AsyncMetricSender;

/// Global async metrics sender (initialized once at startup)
static ASYNC_METRICS_SENDER: OnceLock<AsyncMetricSender> = OnceLock::new();

/// Set the global async metrics sender (called during initialization)
pub fn set_async_metrics_sender(sender: AsyncMetricSender) -> Result<(), AsyncMetricSender> {
    ASYNC_METRICS_SENDER.set(sender)
}

/// Get the global async metrics sender
pub fn get_async_metrics_sender() -> Option<&'static AsyncMetricSender> {
    ASYNC_METRICS_SENDER.get()
}

/// Internal - Generate async functions to record metrics.
#[macro_export]
#[doc(hidden)]
macro_rules! async_metrics_impl_fn_inc {
    (counter $name:ident $group:ident $($label:ident)*) => {
        paste::paste! {
            #[allow(clippy::too_many_arguments)]
            #[doc = "Add n to `" $name "` counter (async)."]
            pub fn [<inc_n_ $name>](n: u64, $( $label: impl Into<super::MetricLabelValue> ),*) {
                use $crate::infra::metrics::metrics_macros::get_async_metrics_sender;
                use $crate::infra::metrics::metrics_collector::{MetricMessage, create_labels};

                let sender = get_async_metrics_sender().expect("Async metrics sender not initialized");
                let labels = create_labels::<super::MetricLabelValue>(vec![
                    ("group", stringify!($group).into()),
                    ("node_mode", $crate::globals::GlobalState::get_node_mode().to_string().into()),
                    $(
                        (stringify!($label), $label.into()),
                    )*
                ]);

                let metric = MetricMessage::counter(stringify!([<stratus_$name>]), labels, n);
                sender.try_send_metric(metric);
            }
        }

        paste::paste! {
            #[allow(clippy::too_many_arguments)]
            #[doc = "Add 1 to `" $name "` counter (async)."]
            pub fn [<inc_ $name>]($( $label: impl Into<super::MetricLabelValue> ),*) {
                [<inc_n_ $name>](1, $($label),*);
            }
        }
    };

    (histogram_counter $name:ident $group:ident $($label:ident)*) => {
        paste::paste! {
            #[allow(clippy::too_many_arguments)]
            #[doc = "Add N to `" $name "` histogram (async)."]
            pub fn [<inc_ $name>](n: usize, $( $label: impl Into<super::MetricLabelValue> ),*) {
                use $crate::infra::metrics::metrics_macros::get_async_metrics_sender;
                use $crate::infra::metrics::metrics_collector::{MetricMessage, create_labels};

                let sender = get_async_metrics_sender().expect("Async metrics sender not initialized");
                let labels = create_labels::<super::MetricLabelValue>(vec![
                    ("group", stringify!($group).into()),
                    ("node_mode", $crate::globals::GlobalState::get_node_mode().to_string().into()),
                    $(
                        (stringify!($label), $label.into()),
                    )*
                ]);

                let metric = MetricMessage::histogram(stringify!([<stratus_$name>]), labels, n as f64);
                sender.try_send_metric(metric);
            }
        }
    };

    (histogram_duration $name:ident $group:ident $($label:ident)*) => {
        paste::paste! {
            #[allow(clippy::too_many_arguments)]
            #[doc = "Add operation duration to `" $name "` histogram (async)."]
            pub fn [<inc_ $name>](duration: std::time::Duration, $( $label: impl Into<super::MetricLabelValue> ),*) {
                use $crate::infra::metrics::metrics_macros::get_async_metrics_sender;
                use $crate::infra::metrics::metrics_collector::{MetricMessage, create_labels};

                let sender = get_async_metrics_sender().expect("Async metrics sender not initialized");
                let labels = create_labels::<super::MetricLabelValue>(vec![
                    ("group", stringify!($group).into()),
                    ("node_mode", $crate::globals::GlobalState::get_node_mode().to_string().into()),
                    $(
                        (stringify!($label), $label.into()),
                    )*
                ]);

                let metric = MetricMessage::histogram(stringify!([<stratus_$name>]), labels, duration.as_secs_f64());
                sender.try_send_metric(metric);
            }
        }
    };

    (gauge $name:ident $group:ident $($label:ident)*) => {
        paste::paste! {
            #[allow(clippy::too_many_arguments)]
            #[doc = "Set `" $name "` gauge (async)."]
            pub fn [<set_ $name>](n: u64, $( $label: impl Into<super::MetricLabelValue> ),*) {
                use $crate::infra::metrics::metrics_macros::get_async_metrics_sender;
                use $crate::infra::metrics::metrics_collector::{MetricMessage, create_labels};

                let sender = get_async_metrics_sender().expect("Async metrics sender not initialized");
                let labels = create_labels::<super::MetricLabelValue>(vec![
                    ("group", stringify!($group).into()),
                    ("node_mode", $crate::globals::GlobalState::get_node_mode().to_string().into()),
                    $(
                        (stringify!($label), $label.into()),
                    )*
                ]);

                let metric = MetricMessage::gauge(stringify!([<stratus_$name>]), labels, n as f64);
                sender.try_send_metric(metric);
            }
        }
    };
}

/// Generate async metrics functions
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
        // Generate function to get metric definition
        paste::paste! {
            // Generate constant to access by name.
            $(
                pub const [<METRIC_ $name:upper>]: &str = stringify!([<stratus_ $name>]);
            )+

            // Generate function that return metric definition.
            pub(super) fn [<metrics_for_ $group>]() -> Vec<super::Metric> {
                vec![
                    $(
                        super::Metric {
                            kind: stringify!($kind),
                            name: stringify!([<stratus_ $name>]),
                            description: stringify!($description),
                        },
                    )+
                ]
            }
        }

        // Generate async function to record metrics values.
        $(
            $crate::async_metrics_impl_fn_inc!($kind $name $group $($label)*);
        )+
    }
}
