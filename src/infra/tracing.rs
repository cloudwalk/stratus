//! Tracing services.

use tracing_subscriber::EnvFilter;

/// Init application global tracing.
pub fn init_tracing() {
    println!("starting tracing");

    // if tracing level not configured, set default
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "stratus=debug");
    }

    tracing_subscriber::fmt()
        .compact()
        .with_target(false)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_env_filter(EnvFilter::from_default_env())
        .try_init()
        .expect("failed to start tracing");

    tracing::info!("started tracing");
}
