/// Internal - Generate functions to record metrics.
#[macro_export]
#[doc(hidden)]
macro_rules! metrics {
    (
        group: $group:ident,
        $(
            $description:literal $(@ sample($rate:expr))?
            $kind:ident $name:ident{ $($label:ident),* }
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
                        },
                    )+
                ]
            }
        }

        // Generate function to record metrics values.
        $(
            $crate::metrics_impl_fn_inc!($kind $name $group $($label)* $(; sample_rate = $rate)?);
        )+
    }
}

/// Internal - Generates a function that increases a metric value.
#[macro_export]
#[doc(hidden)]
macro_rules! metrics_impl_fn_inc {
    // Counter with sampling
    (counter $name:ident $group:ident $($label:ident)* ; sample_rate = $rate:expr) => {
        paste::paste! {
            #[allow(clippy::too_many_arguments)]
            #[doc = "Add n to `" $name "` counter."]
            pub fn [<inc_n_ $name>](n: u64, $( $label: impl Into<super::MetricLabelValue> ),*) {
                if !super::should_sample($rate) {
                    return;
                }
                
                let labels = super::into_labels(
                    vec![
                        ("group", stringify!($group).into()),
                        ("node_mode", $crate::globals::GlobalState::get_node_mode().to_string().into()),

                        $(
                            (stringify!($label), $label.into()),
                        )*
                    ]
                );
                let counter = metrics::counter!(stringify!([<stratus_$name>]), labels);
                counter.increment(n);
            }
        }

        paste::paste! {
            #[allow(clippy::too_many_arguments)]
            #[doc = "Add 1 to `" $name "` counter."]
            pub fn [<inc_ $name>]($( $label: impl Into<super::MetricLabelValue> ),*) {
                if !super::should_sample($rate) {
                    return;
                }
                
                let labels = super::into_labels(
                    vec![
                        ("group", stringify!($group).into()),
                        ("node_mode", $crate::globals::GlobalState::get_node_mode().to_string().into()),

                        $(
                            (stringify!($label), $label.into()),
                        )*
                    ]
                );
                let counter = metrics::counter!(stringify!([<stratus_$name>]), labels);
                counter.increment(1);
            }
        }
    };
    
    // Counter without sampling (default behavior)
    (counter $name:ident $group:ident $($label:ident)*) => {
        paste::paste! {
            #[allow(clippy::too_many_arguments)]
            #[doc = "Add n to `" $name "` counter."]
            pub fn [<inc_n_ $name>](n: u64, $( $label: impl Into<super::MetricLabelValue> ),*) {
                let labels = super::into_labels(
                    vec![
                        ("group", stringify!($group).into()),
                        ("node_mode", $crate::globals::GlobalState::get_node_mode().to_string().into()),

                        $(
                            (stringify!($label), $label.into()),
                        )*
                    ]
                );
                let counter = metrics::counter!(stringify!([<stratus_$name>]), labels);
                counter.increment(n);
            }
        }

        paste::paste! {
            #[allow(clippy::too_many_arguments)]
            #[doc = "Add 1 to `" $name "` counter."]
            pub fn [<inc_ $name>]($( $label: impl Into<super::MetricLabelValue> ),*) {
                let labels = super::into_labels(
                    vec![
                        ("group", stringify!($group).into()),
                        ("node_mode", $crate::globals::GlobalState::get_node_mode().to_string().into()),

                        $(
                            (stringify!($label), $label.into()),
                        )*
                    ]
                );
                let counter = metrics::counter!(stringify!([<stratus_$name>]), labels);
                counter.increment(1);
            }
        }
    };
    
    // Histogram counter with sampling
    (histogram_counter $name:ident $group:ident $($label:ident)* ; sample_rate = $rate:expr) => {
        paste::paste! {
            #[allow(clippy::too_many_arguments)]
            #[doc = "Add N to `" $name "` histogram."]
            pub fn [<inc_ $name>](n: usize, $( $label: impl Into<super::MetricLabelValue> ),*) {
                if !super::should_sample($rate) {
                    return;
                }
                
                let labels = super::into_labels(
                    vec![
                        ("group", stringify!($group).into()),
                        ("node_mode", $crate::globals::GlobalState::get_node_mode().to_string().into()),

                        $(
                            (stringify!($label), $label.into()),
                        )*
                    ]
                );
                let hist = metrics::histogram!(stringify!([<stratus_$name>]), labels);
                hist.record(n as f64)
            }
        }
    };
    
    // Histogram counter without sampling (default behavior)
    (histogram_counter $name:ident $group:ident $($label:ident)*) => {
        paste::paste! {
            #[allow(clippy::too_many_arguments)]
            #[doc = "Add N to `" $name "` histogram."]
            pub fn [<inc_ $name>](n: usize, $( $label: impl Into<super::MetricLabelValue> ),*) {
                let labels = super::into_labels(
                    vec![
                        ("group", stringify!($group).into()),
                        ("node_mode", $crate::globals::GlobalState::get_node_mode().to_string().into()),

                        $(
                            (stringify!($label), $label.into()),
                        )*
                    ]
                );
                let hist = metrics::histogram!(stringify!([<stratus_$name>]), labels);
                hist.record(n as f64)
            }
        }
    };
    
    // Histogram duration with sampling
    (histogram_duration $name:ident $group:ident $($label:ident)* ; sample_rate = $rate:expr) => {
        paste::paste! {
            #[allow(clippy::too_many_arguments)]
            #[doc = "Add operation duration to `" $name "` histogram."]
            pub fn [<inc_ $name>](duration: std::time::Duration, $( $label: impl Into<super::MetricLabelValue> ),*) {
                if !super::should_sample($rate) {
                    return;
                }
                
                let labels = super::into_labels(
                    vec![
                        ("group", stringify!($group).into()),
                        ("node_mode", $crate::globals::GlobalState::get_node_mode().to_string().into()),

                        $(
                            (stringify!($label), $label.into()),
                        )*
                    ]
                );
                let hist = metrics::histogram!(stringify!([<stratus_$name>]), labels);
                hist.record(duration);
            }
        }
    };
    
    // Histogram duration without sampling (default behavior)
    (histogram_duration $name:ident $group:ident $($label:ident)*) => {
        paste::paste! {
            #[allow(clippy::too_many_arguments)]
            #[doc = "Add operation duration to `" $name "` histogram."]
            pub fn [<inc_ $name>](duration: std::time::Duration, $( $label: impl Into<super::MetricLabelValue> ),*) {
                let labels = super::into_labels(
                    vec![
                        ("group", stringify!($group).into()),
                        ("node_mode", $crate::globals::GlobalState::get_node_mode().to_string().into()),

                        $(
                            (stringify!($label), $label.into()),
                        )*
                    ]
                );
                let hist = metrics::histogram!(stringify!([<stratus_$name>]), labels);
                hist.record(duration);
            }
        }
    };
    
    // Gauge with sampling
    (gauge $name:ident $group:ident $($label:ident)* ; sample_rate = $rate:expr) => {
        paste::paste! {
            #[allow(clippy::too_many_arguments)]
            #[doc = "Set `" $name "` gauge."]
            pub fn [<set_ $name>](n: u64, $( $label: impl Into<super::MetricLabelValue> ),*) {
                if !super::should_sample($rate) {
                    return;
                }
                
                let labels = super::into_labels(
                    vec![
                        ("group", stringify!($group).into()),
                        ("node_mode", $crate::globals::GlobalState::get_node_mode().to_string().into()),

                        $(
                            (stringify!($label), $label.into()),
                        )*
                    ]
                );
                let gauge = metrics::gauge!(stringify!([<stratus_$name>]), labels);
                gauge.set(n as f64);
            }
        }
    };
    
    // Gauge without sampling (default behavior)
    (gauge $name:ident $group:ident $($label:ident)*) => {
        paste::paste! {
            #[allow(clippy::too_many_arguments)]
            #[doc = "Set `" $name "` gauge."]
            pub fn [<set_ $name>](n: u64, $( $label: impl Into<super::MetricLabelValue> ),*) {
                let labels = super::into_labels(
                    vec![
                        ("group", stringify!($group).into()),
                        ("node_mode", $crate::globals::GlobalState::get_node_mode().to_string().into()),

                        $(
                            (stringify!($label), $label.into()),
                        )*
                    ]
                );
                let gauge = metrics::gauge!(stringify!([<stratus_$name>]), labels);
                gauge.set(n as f64);
            }
        }
    };
}
