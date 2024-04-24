use sentry::ClientInitGuard;

pub fn init_sentry(url: &str) -> ClientInitGuard {
    sentry::init((
        url,
        sentry::ClientOptions {
            release: sentry::release_name!(),
            ..Default::default()
        },
    ))
}
