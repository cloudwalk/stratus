use sentry::ClientInitGuard;

use crate::config::Environment;
use crate::ext::not;
use crate::infra::build_info;

pub fn init_sentry(url: &str, env: Environment) -> anyhow::Result<ClientInitGuard> {
    let release = build_info::service_name_with_version();
    tracing::info!(%url, %env, %release, "creating sentry exporter");

    let guard = sentry::init((
        url,
        sentry::ClientOptions {
            release: Some(release.clone().into()),
            environment: Some(env.to_string().into()),
            ..Default::default()
        },
    ));
    if not(guard.is_enabled()) {
        tracing::error!(%url, %env, %release, "failed to create sentry exporter");
    }

    Ok(guard)
}
