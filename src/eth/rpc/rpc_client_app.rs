use std::fmt::Display;

#[cfg(feature = "metrics")]
use crate::infra::metrics::MetricLabelValue;

// Hardcoded scope configuration
const CLIENT_SCOPES_CONFIG: &str = include_str!("client_scopes.txt");

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
    /// Parse scope configuration and return matching scope and trim info for a given name.
    fn find_scope_for_name(name: &str) -> (String, Option<String>) {
        for line in CLIENT_SCOPES_CONFIG.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() != 2 {
                continue;
            }
            
            let scope = parts[0].trim();
            let patterns = parts[1].trim();
            
            // Check if name matches any pattern for this scope
            if let Some(matched_pattern) = Self::find_matching_pattern(name, patterns) {
                return (scope.to_string(), Some(matched_pattern));
            }
        }
        
        // Default fallback
        ("other".to_string(), None)
    }
    
    /// Find the specific pattern that matches the name and return it.
    fn find_matching_pattern(name: &str, patterns: &str) -> Option<String> {
        let pattern_parts: Vec<&str> = patterns.split_whitespace().collect();
        
        for pattern in pattern_parts {
            if pattern.ends_with("/") {
                let prefix = &pattern[..pattern.len() - 1];
                if name.starts_with(prefix) {
                    return Some(pattern.to_string());
                }
            } else if pattern.ends_with('*') {
                let prefix = &pattern[..pattern.len() - 1];
                if name.starts_with(prefix) {
                    return Some(pattern.to_string());
                }
            } else {
                // Exact match with pattern
                if name == pattern {
                    return Some(pattern.to_string());
                }
            }
        }
        
        None
    }


    /// Parse known client application name to groups.
    pub fn parse(name: &str) -> RpcClientApp {
        let name = name.trim().trim_start_matches('/').trim_end_matches('/').to_ascii_lowercase().replace('_', "-");
        if name.is_empty() {
            return RpcClientApp::Unknown;
        }
        let (scope, matched_pattern) = Self::find_scope_for_name(&name);
        
        let formatted_name = if let Some(pattern) = matched_pattern {
            if pattern.ends_with("/") {
                let prefix = &pattern[..pattern.len() - 1];
                let suffix = name.trim_start_matches(prefix);
                format!("{}::{}", scope, suffix)
            } else {
                format!("{}::{}", scope, name)
            }
        } else {
            format!("{}::{}", scope, name)
        };
        
        RpcClientApp::Identified(formatted_name)
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
        
        // Test other scope (fallback)
        assert_eq!(RpcClientApp::parse("random-service").to_string(), "other::random-service");
    }
}
