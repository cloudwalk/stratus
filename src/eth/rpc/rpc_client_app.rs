use std::fmt::Display;

#[cfg(feature = "metrics")]
use crate::infra::metrics::MetricLabelValue;

// Include the build-time generated client scopes matcher
include!(concat!(env!("OUT_DIR"), "/client_scopes.rs"));

#[derive(Debug, Clone, strum::EnumIs, PartialEq, Eq, Hash)]
pub enum RpcClientApp {
    /// Client application identified itself.
    Identified(String),

    /// Client application is unknown.
    Unknown,
}

impl Display for RpcClientApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RpcClientApp::Identified(name) => write!(f, "{name}"),
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

        RpcClientApp::Identified(create_client_scope(&name))
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

impl<'de> serde::Deserialize<'de> for RpcClientApp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "unknown" => Ok(Self::Unknown),
            _ => Ok(Self::Identified(value)),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_parsing() {
        // Test stratus scope (stratus*/ - prefix trimmed)
        assert_eq!(RpcClientApp::parse("stratus-node").to_string(), "stratus::-node");
        assert_eq!(RpcClientApp::parse("stratus").to_string(), "stratus::");

        // Test acquiring scope (exact match: authorizer)
        assert_eq!(RpcClientApp::parse("authorizer").to_string(), "acquiring::authorizer");

        // Test banking scope (balance* and banking* - prefix NOT trimmed)
        assert_eq!(RpcClientApp::parse("banking-service").to_string(), "banking::banking-service");
        assert_eq!(RpcClientApp::parse("balance-checker").to_string(), "banking::balance-checker");

        // Test issuing scope (issuing* and infinitecard* - prefix NOT trimmed)
        assert_eq!(RpcClientApp::parse("issuing-api").to_string(), "issuing::issuing-api");
        assert_eq!(RpcClientApp::parse("infinitecard-service").to_string(), "issuing::infinitecard-service");

        // Test lending scope (lending* - prefix NOT trimmed)
        assert_eq!(RpcClientApp::parse("lending-core").to_string(), "lending::lending-core");

        // Test infra scope (exact matches: blockscout, golani, tx-replayer)
        assert_eq!(RpcClientApp::parse("blockscout").to_string(), "infra::blockscout");
        assert_eq!(RpcClientApp::parse("golani").to_string(), "infra::golani");
        assert_eq!(RpcClientApp::parse("tx-replayer").to_string(), "infra::tx-replayer");

        // Test user scope (user-*/ - prefix trimmed, OR exact match: insomnia)
        assert_eq!(RpcClientApp::parse("user-service").to_string(), "user::service");
        assert_eq!(RpcClientApp::parse("insomnia").to_string(), "user::insomnia");

        // Test exact match edge cases - these should fall through to "other" scope
        // Names similar to exact matches but with extra information
        assert_eq!(RpcClientApp::parse("authorizer-v2").to_string(), "other::authorizer-v2");
        assert_eq!(RpcClientApp::parse("authorizer.staging").to_string(), "other::authorizer.staging");
        assert_eq!(RpcClientApp::parse("super-authorizer").to_string(), "other::super-authorizer");

        assert_eq!(RpcClientApp::parse("blockscout-api").to_string(), "other::blockscout-api");
        assert_eq!(RpcClientApp::parse("blockscout-staging").to_string(), "other::blockscout-staging");
        assert_eq!(RpcClientApp::parse("mini-blockscout").to_string(), "other::mini-blockscout");

        assert_eq!(RpcClientApp::parse("golani-dev").to_string(), "other::golani-dev");
        assert_eq!(RpcClientApp::parse("golani.local").to_string(), "other::golani.local");
        assert_eq!(RpcClientApp::parse("pre-golani").to_string(), "other::pre-golani");

        assert_eq!(RpcClientApp::parse("tx-replayer-v3").to_string(), "other::tx-replayer-v3");
        assert_eq!(RpcClientApp::parse("tx-replayer.backup").to_string(), "other::tx-replayer.backup");
        assert_eq!(RpcClientApp::parse("new-tx-replayer").to_string(), "other::new-tx-replayer");

        assert_eq!(RpcClientApp::parse("insomnia-client").to_string(), "other::insomnia-client");
        assert_eq!(RpcClientApp::parse("insomnia.v8").to_string(), "other::insomnia.v8");
        assert_eq!(RpcClientApp::parse("custom-insomnia").to_string(), "other::custom-insomnia");

        // Test other scope (fallback)
        assert_eq!(RpcClientApp::parse("random-service").to_string(), "other::random-service");
    }
}
