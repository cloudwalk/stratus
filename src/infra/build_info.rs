use serde_json::json;

// -----------------------------------------------------------------------------
// Build constants
// -----------------------------------------------------------------------------
pub const BUILD_TIMESTAMP: &str = env!("VERGEN_BUILD_TIMESTAMP");

pub const CARGO_DEBUG: &str = env!("VERGEN_CARGO_DEBUG");
pub const CARGO_FEATURES: &str = env!("VERGEN_CARGO_FEATURES");

pub const GIT_COMMIT: &str = env!("VERGEN_GIT_SHA");
pub const GIT_BRANCH: &str = env!("VERGEN_GIT_BRANCH");

pub const RUSTC_VERSION: &str = env!("VERGEN_RUSTC_SEMVER");
pub const RUSTC_CHANNEL: &str = env!("VERGEN_RUSTC_CHANNEL");
pub const RUSTC_TARGET: &str = env!("VERGEN_RUSTC_HOST_TRIPLE");

const VERSION: &str = const_format::formatcp!("{}::{}", GIT_BRANCH, GIT_COMMIT);

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
    VERSION
}

/// Returns build info as JSON.
pub fn as_json() -> serde_json::Value {
    json!(
        {
            "_service_name": service_name(),
            "_binary_name": binary_name(),
            "_version": version(),
            "_service_name_with_version": service_name_with_version(),
            "build_timestamp": BUILD_TIMESTAMP,
            "cargo_debug": CARGO_DEBUG,
            "cargo_features": CARGO_FEATURES,
            "git_commit": GIT_COMMIT,
            "git_branch": GIT_BRANCH,
            "rustc_version": RUSTC_VERSION,
            "rustc_channel": RUSTC_CHANNEL,
            "rustc_target": RUSTC_TARGET
        }
    )
}
