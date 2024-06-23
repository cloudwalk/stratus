use serde_json::json;

// -----------------------------------------------------------------------------
// Build constants
// -----------------------------------------------------------------------------
pub const BUILD_TIMESTAMP: &str = env!("VERGEN_BUILD_TIMESTAMP");

pub const CARGO_DEBUG: &str = env!("VERGEN_CARGO_DEBUG");
pub const CARGO_FEATURES: &str = env!("VERGEN_CARGO_FEATURES");

pub const GIT_COMMIT: &str = env!("VERGEN_GIT_SHA");
pub const GIT_COMMIT_TIMESTAMP: &str = env!("VERGEN_GIT_COMMIT_TIMESTAMP");
pub const GIT_COMMIT_MESSAGE: &str = env!("VERGEN_GIT_COMMIT_MESSAGE");
pub const GIT_COMMIT_AUTHOR: &str = env!("VERGEN_GIT_COMMIT_AUTHOR_NAME");
pub const GIT_BRANCH: &str = env!("VERGEN_GIT_BRANCH");
pub const GIT_DESCRIBE: &str = env!("VERGEN_GIT_DESCRIBE");

pub const RUST_VERSION: &str = env!("VERGEN_RUSTC_SEMVER");
pub const RUST_CHANNEL: &str = env!("VERGEN_RUSTC_CHANNEL");
pub const RUST_TARGET: &str = env!("VERGEN_RUSTC_HOST_TRIPLE");

const VERSION_WITH_BRANCH: &str = const_format::formatcp!("{}::{}", GIT_BRANCH, GIT_COMMIT);
const VERSION_WITH_DESCRIBE: &str = const_format::formatcp!("{}::{}", GIT_DESCRIBE, GIT_COMMIT);

/// Returns the current service name.
///
/// The service name is used to identify the application in external services.
pub fn service_name() -> String {
    format!("stratus-{}", binary_name())
}

/// Returns the current service name with version.
pub fn service_name_with_version() -> String {
    format!("{}::{}", service_name(), version())
}

/// Returns the current binary basename.
pub fn binary_name() -> String {
    let binary = std::env::current_exe().unwrap();
    let binary_basename = binary.file_name().unwrap().to_str().unwrap().to_lowercase();

    if binary_basename.starts_with("test_") {
        "tests".to_string()
    } else {
        binary_basename
    }
}

/// Returns the version derived from the build information.
pub fn version() -> &'static str {
    if GIT_BRANCH == "HEAD" {
        VERSION_WITH_DESCRIBE
    } else {
        VERSION_WITH_BRANCH
    }
}

/// Returns build info as JSON.
pub fn as_json() -> serde_json::Value {
    json!(
        {
            "build": {
                "service_name": service_name(),
                "binary_name": binary_name(),
                "version": version(),
                "service_name_with_version": service_name_with_version(),
                "timestamp": BUILD_TIMESTAMP,
            },
            "cargo": {
                "debug": CARGO_DEBUG,
                "features": CARGO_FEATURES,
            },
            "git": {
                "commit": GIT_COMMIT,
                "commit_timestamp": GIT_COMMIT_TIMESTAMP,
                "commit_message": GIT_COMMIT_MESSAGE,
                "commit_author": GIT_COMMIT_AUTHOR,
                "branch": GIT_BRANCH,
                "describe": GIT_DESCRIBE,
            },
            "rust": {
                "version": RUST_VERSION,
                "channel": RUST_CHANNEL,
                "target": RUST_TARGET
            }
        }
    )
}
