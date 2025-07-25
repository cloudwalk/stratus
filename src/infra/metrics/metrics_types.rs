use std::borrow::Cow;
use std::time::Duration;

use metrics::Label;
use metrics::describe_counter;
use metrics::describe_gauge;
use metrics::describe_histogram;

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
// Timed
// -----------------------------------------------------------------------------
#[cfg(feature = "metrics")]
/// Measures how long the provided function takes to execute.
///
/// Returns a wrapper that allows to using it to record metrics if the `metrics` feature is enabled.
pub fn timed<F, T>(f: F) -> Timed<T>
where
    F: FnOnce() -> T,
{
    let start = crate::infra::metrics::now();
    let result = f();
    Timed {
        elapsed: start.elapsed(),
        result,
    }
}

#[cfg(not(feature = "metrics"))]
/// Executes the provided function
pub fn timed<F, T>(f: F) -> Timed<T>
where
    F: FnOnce() -> T,
{
    let result = f();
    Timed {
        elapsed: Duration::default(),
        result,
    }
}

pub struct Timed<T> {
    pub elapsed: Duration,
    pub result: T,
}

impl<T> Timed<T> {
    #[cfg(feature = "metrics")]
    #[inline(always)]
    /// Applies the provided function to the current metrified execution.
    pub fn with<F>(self, f: F) -> T
    where
        F: FnOnce(&Timed<T>),
    {
        f(&self);
        self.result
    }

    #[cfg(not(feature = "metrics"))]
    #[inline(always)]
    /// Do nothing because the `metrics` function is disabled.
    pub fn with<F>(self, _: F) -> T
    where
        F: FnOnce(&Timed<T>),
    {
        self.result
    }
}
