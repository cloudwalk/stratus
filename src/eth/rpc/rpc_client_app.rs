use std::fmt::Display;

#[cfg(feature = "metrics")]
use crate::infra::metrics::MetricLabelValue;

#[derive(Debug, Clone, Default, strum::EnumIs, PartialEq)]
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

impl RpcClientApp {
    /// Parse known client application name to groups.
    pub fn parse(name: &str) -> RpcClientApp {
        let name = name.trim().trim_start_matches('/').trim_end_matches('/').to_ascii_lowercase().replace('_', "-");
        if name.is_empty() {
            return RpcClientApp::Unknown;
        }
        let name = match name {
            v if v.starts_with("banking") || v.starts_with("balance") => format!("banking::{}", v),
            v if v.starts_with("issuing") || v.starts_with("infinitecard") => format!("issuing::{}", v),
            v if v.starts_with("lending") => format!("lending::{}", v),
            v if v == "blockscout" || v == "golani" || v == "tx-replayer" => format!("infra::{}", v),
            v if v.starts_with("user-") => {
                let v = v.trim_start_matches("user-");
                format!("user::{}", v)
            }
            v => format!("other::{}", v),
        };
        RpcClientApp::Identified(name)
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
