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
                            buckets: $buckets.to_vec()
                        },
                    )+
                ]
            }
        }

        // Generate function to record metrics values.
        $(
            $crate::metrics_impl_fn_inc!($kind $name $group $($label)*);
        )+
    }
}

/// Internal - Generates a function that increases a metric value.
#[macro_export]
#[doc(hidden)]
macro_rules! metrics_impl_fn_inc {
    (counter $name:ident $group:ident $($label:ident)*) => {
        paste::paste! {
            #[doc = "Add n to `" $name "` counter."]
            pub fn [<inc_n_ $name>](n: u64, $( $label: impl Into<super::MetricLabelValue> ),*) {
                let labels = super::into_labels(
                    vec![
                        ("group", stringify!($group).into()),
                        $(
                            (stringify!($label), $label.into()),
                        )*
                    ]
                );
                metrics::counter!(stringify!([<stratus_$name>]), n, labels);
            }
        }

        paste::paste! {
            #[doc = "Add 1 to `" $name "` counter."]
            pub fn [<inc_ $name>]($( $label: impl Into<super::MetricLabelValue> ),*) {
                let labels = super::into_labels(
                    vec![
                        ("group", stringify!($group).into()),
                        $(
                            (stringify!($label), $label.into()),
                        )*
                    ]
                );
                metrics::counter!(stringify!([<stratus_$name>]), 1, labels);
            }
        }
    };
    (histogram_counter  $name:ident $group:ident $($label:ident)*) => {
        paste::paste! {
            #[doc = "Add N to `" $name "` histogram."]
            pub fn [<inc_ $name>](n: usize, $( $label: impl Into<super::MetricLabelValue> ),*) {
                let labels = super::into_labels(
                    vec![
                        ("group", stringify!($group).into()),
                        $(
                            (stringify!($label), $label.into()),
                        )*
                    ]
                );
                metrics::histogram!(stringify!([<stratus_$name>]), n as f64, labels);
            }
        }
    };
    (histogram_duration  $name:ident $group:ident $($label:ident)*) => {
        paste::paste! {
            #[doc = "Add operation duration to `" $name "` histogram."]
            pub fn [<inc_ $name>](duration: std::time::Duration, $( $label: impl Into<super::MetricLabelValue> ),*) {
                let labels = super::into_labels(
                    vec![
                        ("group", stringify!($group).into()),
                        $(
                            (stringify!($label), $label.into()),
                        )*
                    ]
                );
                metrics::histogram!(stringify!([<stratus_$name>]), duration, labels);
            }
        }
    };
    (gauge  $name:ident $group:ident $($label:ident)*) => {
        paste::paste! {
            #[doc = "Set `" $name "` gauge."]
            pub fn [<set_ $name>](n: u64, $( $label: impl Into<super::MetricLabelValue> ),*) {
                let labels = super::into_labels(
                    vec![
                        ("group", stringify!($group).into()),
                        $(
                            (stringify!($label), $label.into()),
                        )*
                    ]
                );
                metrics::gauge!(stringify!([<stratus_$name>]), n as f64, labels);
            }
        }
    };
}
