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
// Serialization / Deserialization
// -----------------------------------------------------------------------------
impl serde::Serialize for RpcClientApp {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            RpcClientApp::Identified(client) => serializer.serialize_str(client.as_ref()),
            RpcClientApp::Unknown => serializer.serialize_str("unknown"),
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
