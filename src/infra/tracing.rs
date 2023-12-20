//! Tracing configuration.

use tracing_subscriber::EnvFilter;

/// Init application tracing.
pub fn init_tracing() {
    // if tracing level not configured, set default
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "ledger=debug");
    }

    tracing_subscriber::fmt()
        .compact()
        .with_target(false)
        .with_env_filter(EnvFilter::from_default_env())
        .try_init()
        .expect("Tracing initialization failed");

    tracing::info!("tracing initialized");
}
