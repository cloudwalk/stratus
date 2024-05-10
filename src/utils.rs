use tokio::select;
use tokio::signal::unix::signal;
use tokio::signal::unix::SignalKind;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub fn new_context_id() -> String {
    Uuid::new_v4().to_string()
}

pub fn signal_handler() -> CancellationToken {
    let cancellation = CancellationToken::new();
    let task_cancellation = cancellation.clone();
    tokio::spawn(async move {
        let mut sigterm = signal(SignalKind::terminate()).expect("unable to listen for SIGTERM");
        let mut sigint = signal(SignalKind::interrupt()).expect("unable to listen for SIGTERM");
        select! {
            _ = sigterm.recv() => {
                tracing::info!("SIGTERM received, cancelling tasks");
            }

            _ = sigint.recv() => {
                tracing::info!("SIGINT signal received, shutting down");
            }
        }
        task_cancellation.cancel();
    });
    cancellation
}
