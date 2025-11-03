mod sentry_config;

pub use sentry_config::SentryConfig;

pub fn sentry_event_filter(metadata: &tracing::Metadata) -> sentry_tracing::EventFilter {
    use sentry_tracing::EventFilter;
    use tracing::Level;

    match *metadata.level() {
        Level::ERROR => EventFilter::Event,
        _ => EventFilter::Ignore,
    }
}

pub fn sentry_span_filter(metadata: &tracing::Metadata) -> bool {
    matches!(*metadata.level(), tracing::Level::ERROR)
}
