use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub fn new_context_id() -> String {
    Uuid::new_v4().to_string()
}

pub fn signal_handler() -> CancellationToken {
    let cancellation = CancellationToken::new();
    let task_cancellation = cancellation.clone();
    tokio::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                tracing::info!("stop signal received, shutting down");
                task_cancellation.cancel();
            }
            Err(err) => tracing::error!("unable to listen for shutdown signal: {}", err),
        }
    });
    cancellation
}
