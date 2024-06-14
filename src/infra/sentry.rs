use sentry::ClientInitGuard;

use crate::config::Environment;
use crate::ext::not;
use crate::infra::build_info;

pub fn init_sentry(url: &str, env: Environment) -> anyhow::Result<ClientInitGuard> {
    tracing::info!(%url, %env, "creating sentry exporter");

    let guard = sentry::init((
        url,
        sentry::ClientOptions {
            release: Some(build_info::service_name_with_version().into()),
            environment: Some(env.to_string().into()),
            ..Default::default()
        },
    ));
    if not(guard.is_enabled()) {
        tracing::error!(%url, "failed to create sentry exporter");
    }

    Ok(guard)
}
