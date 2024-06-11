use std::borrow::Cow;
use std::str::FromStr;

use anyhow::anyhow;
use display_json::DebugAsJson;
use metrics::describe_counter;
use metrics::describe_gauge;
use metrics::describe_histogram;
use metrics::Label;

use crate::ext::not;

// -----------------------------------------------------------------------------
// Metric
// -----------------------------------------------------------------------------

/// Metric definition.
pub(super) struct Metric {
    pub(super) kind: &'static str,
    pub(super) name: &'static str,
    pub(super) description: &'static str,
    pub(super) buckets: Vec<f64>,
}

impl Metric {
    /// Checks if metric has custom buckets defined.
    pub(super) fn has_custom_buckets(&self) -> bool {
        not(self.buckets.is_empty())
    }

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

impl From<Option<Cow<'static, str>>> for MetricLabelValue {
    fn from(value: Option<Cow<'static, str>>) -> Self {
        match value {
            Some(str) => Self::Some(str.into_owned()),
            None => Self::None,
        }
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

// -----------------------------------------------------------------------------
// MetricsHistogramKind
// -----------------------------------------------------------------------------

/// See: <https://prometheus.io/docs/practices/histograms/>
#[derive(DebugAsJson, Clone, Copy, Eq, PartialEq, serde::Serialize)]
pub enum MetricsHistogramKind {
    /// Quantiles are calculated on client-side based on recent data kept in-memory.
    ///
    /// Client defines the quantiles to calculate.
    Summary,

    /// Quantiles are calculated on server-side based on bucket counts.
    ///
    /// Cient defines buckets to group observations.
    Histogram,
}

impl FromStr for MetricsHistogramKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self, Self::Err> {
        match s.to_lowercase().trim() {
            "summary" => Ok(Self::Summary),
            "histogram" => Ok(Self::Histogram),
            s => Err(anyhow!("unknown metrics histogram kind: {}", s)),
        }
    }
}
