use std::fmt::Display;

#[cfg(feature = "metrics")]
use crate::infra::metrics::MetricLabelValue;

#[derive(Debug, Clone, Default)]
pub enum RpcClientApp {
    /// Client application identified itself.
    Identified(String),

    /// Client application is unknown.
    #[default]
    Unknown,
}

impl Display for RpcClientApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RpcClientApp::Identified(name) => write!(f, "{}", name),
            RpcClientApp::Unknown => write!(f, "unknown"),
        }
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
#[cfg(feature = "metrics")]
impl From<&RpcClientApp> for MetricLabelValue {
    fn from(value: &RpcClientApp) -> Self {
        match value {
            RpcClientApp::Identified(name) => Self::Some(name.to_string()),
            RpcClientApp::Unknown => Self::Some("unknown".to_string()),
        }
    }
}
