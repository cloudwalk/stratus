use clap::Parser;
use display_json::DebugAsJson;
use sentry::ClientInitGuard;

use crate::config::Environment;
use crate::ext::not;
use crate::infra::build_info;

#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
pub struct SentryConfig {
    /// Sentry server URL.
    #[arg(long = "sentry-url", env = "SENTRY_URL", required = false)]
    pub sentry_url: String,
}

impl SentryConfig {
    /// Inits application global Sentry exporter.
    pub fn init(&self, env: Environment) -> anyhow::Result<ClientInitGuard> {
        let release = build_info::service_name_with_version();
        tracing::info!(url = %self.sentry_url, %env, %release, "creating sentry exporter");

        let guard = sentry::init((
            self.sentry_url.clone(),
            sentry::ClientOptions {
                release: Some(release.clone().into()),
                environment: Some(env.to_string().into()),
                ..Default::default()
            },
        ));
        if not(guard.is_enabled()) {
            tracing::error!(url = %self.sentry_url, %env, %release, "failed to create sentry exporter");
        }

        Ok(guard)
    }
}
