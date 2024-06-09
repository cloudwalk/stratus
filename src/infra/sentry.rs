use sentry::ClientInitGuard;

use crate::ext::not;

pub fn init_sentry(url: &str) -> anyhow::Result<ClientInitGuard> {
    tracing::info!(%url, "creating sentry exporter");

    let guard = sentry::init((
        url,
        sentry::ClientOptions {
            release: sentry::release_name!(),
            ..Default::default()
        },
    ));
    if not(guard.is_enabled()) {
        tracing::error!(%url, "failed to create sentry exporter");
    }

    Ok(guard)
}
