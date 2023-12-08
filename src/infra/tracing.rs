use tracing_subscriber::EnvFilter;

pub fn init_tracing() {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "ledger=debug");
    }
    let _ = tracing_subscriber::fmt().compact().with_env_filter(EnvFilter::from_default_env()).try_init();
}
