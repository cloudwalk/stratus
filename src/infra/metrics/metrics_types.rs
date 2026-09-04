use std::borrow::Cow;

use metrics::Label;
use metrics::describe_counter;
use metrics::describe_gauge;
use metrics::describe_histogram;

use crate::eth::executor::EvmKind;

pub type HistogramInt = u32;
pub type Sum = u64;
pub type Count = u64;

// -----------------------------------------------------------------------------
// Labels
// -----------------------------------------------------------------------------

/// Label value indicating a value is present.
pub const LABEL_PRESENT: &str = "present";

/// Label value indicating a value is missing.
pub const LABEL_MISSING: &str = "missing";

/// Label value indicating a value is unknown.
pub const LABEL_UNKNOWN: &str = "unknown";

/// Label value indicating an error happened.
pub const LABEL_ERROR: &str = "error";

// -----------------------------------------------------------------------------
// Metric
// -----------------------------------------------------------------------------

/// Metric definition.
pub(super) struct Metric {
    pub(super) kind: &'static str,
    pub(super) name: &'static str,
    pub(super) description: &'static str,
}

impl Metric {
    /// Register description with the provider.
    pub(super) fn register_description(&self) {
        match self.kind {
            "counter" => describe_counter!(self.name, self.description),
            "histogram_duration" | "histogram_counter" => describe_histogram!(self.name, self.description),
            "gauge" => describe_gauge!(self.name, self.description),
            _ => {}
        }
    }
}

// -----------------------------------------------------------------------------
// MetricLabelValue
// -----------------------------------------------------------------------------

/// Representation of a metric label value.
///
/// It exists to improve two aspects `metrics` crate does not cover:
/// * Conversion from several types to a label value.
/// * Handling of optional values.
pub enum MetricLabelValue {
    /// Label has a value and should be recorded.
    Some(String),
    /// Label does not have a value and should be ignored.
    None,
}

/// Converts a borrowed function parameter into an owned metric label value.
///
/// This allows instrumentation to prepare labels before the function body
/// consumes its parameters, without cloning the parameters themselves.
pub trait ToMetricLabelValue {
    fn to_metric_label_value(&self) -> MetricLabelValue;
}

impl<T> ToMetricLabelValue for &T
where
    T: ToMetricLabelValue + ?Sized,
{
    fn to_metric_label_value(&self) -> MetricLabelValue {
        (*self).to_metric_label_value()
    }
}

impl ToMetricLabelValue for Option<Cow<'static, str>> {
    fn to_metric_label_value(&self) -> MetricLabelValue {
        match self {
            Some(value) => MetricLabelValue::Some(value.to_string()),
            None => MetricLabelValue::None,
        }
    }
}

impl ToMetricLabelValue for String {
    fn to_metric_label_value(&self) -> MetricLabelValue {
        MetricLabelValue::Some(self.to_owned())
    }
}

impl ToMetricLabelValue for str {
    fn to_metric_label_value(&self) -> MetricLabelValue {
        MetricLabelValue::Some(self.to_owned())
    }
}

impl ToMetricLabelValue for Option<&str> {
    fn to_metric_label_value(&self) -> MetricLabelValue {
        match self {
            Some(value) => MetricLabelValue::Some((*value).to_owned()),
            None => MetricLabelValue::None,
        }
    }
}

impl ToMetricLabelValue for bool {
    fn to_metric_label_value(&self) -> MetricLabelValue {
        MetricLabelValue::Some(self.to_string())
    }
}

impl ToMetricLabelValue for i32 {
    fn to_metric_label_value(&self) -> MetricLabelValue {
        MetricLabelValue::Some(self.to_string())
    }
}

impl ToMetricLabelValue for u64 {
    fn to_metric_label_value(&self) -> MetricLabelValue {
        MetricLabelValue::Some(self.to_string())
    }
}

impl ToMetricLabelValue for EvmKind {
    fn to_metric_label_value(&self) -> MetricLabelValue {
        (*self).into()
    }
}

impl From<Option<Cow<'static, str>>> for MetricLabelValue {
    fn from(value: Option<Cow<'static, str>>) -> Self {
        match value {
            Some(str) => Self::Some(str.into_owned()),
            None => Self::None,
        }
    }
}

impl From<&String> for MetricLabelValue {
    fn from(value: &String) -> Self {
        Self::Some(value.to_owned())
    }
}

impl From<&str> for MetricLabelValue {
    fn from(value: &str) -> Self {
        Self::Some(value.to_owned())
    }
}

impl From<Option<&str>> for MetricLabelValue {
    fn from(value: Option<&str>) -> Self {
        match value {
            Some(value) => Self::Some(value.to_owned()),
            None => Self::None,
        }
    }
}

impl From<String> for MetricLabelValue {
    fn from(value: String) -> Self {
        Self::Some(value)
    }
}

impl From<bool> for MetricLabelValue {
    fn from(value: bool) -> Self {
        Self::Some(value.to_string())
    }
}

impl From<i32> for MetricLabelValue {
    fn from(value: i32) -> Self {
        Self::Some(value.to_string())
    }
}

impl From<u64> for MetricLabelValue {
    fn from(value: u64) -> Self {
        Self::Some(value.to_string())
    }
}

impl From<EvmKind> for MetricLabelValue {
    fn from(value: EvmKind) -> Self {
        let label = match value {
            EvmKind::Transaction => "transaction",
            EvmKind::CallPresent => "call_present",
            EvmKind::CallPast => "call_past",
            EvmKind::Inspect => "inspector",
        };
        Self::Some(label.to_owned())
    }
}

/// Converts a list of label keys-value pairs to `metrics::Label`. Labels with missing values are filtered out.
pub(super) fn into_labels(labels: Vec<(&'static str, MetricLabelValue)>) -> Vec<Label> {
    labels
        .into_iter()
        .filter_map(|(key, value)| match value {
            MetricLabelValue::Some(value) => Some((key, value)),
            MetricLabelValue::None => None,
        })
        .map(|(key, value)| Label::new(key, value))
        .collect()
}
