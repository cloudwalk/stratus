use sentry::ClientInitGuard;

use crate::ext::not;
use crate::log_and_err;

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
        return log_and_err!(format!("failed to create sentry exporter at {}", url));
    }

    Ok(guard)
}
