use anyhow::anyhow;
use tokio::select;
use tokio::signal::unix::signal;
use tokio::signal::unix::Signal;
use tokio::signal::unix::SignalKind;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::ext::spawn_named;

pub fn new_context_id() -> String {
    Uuid::new_v4().to_string()
}

fn signal_or_cancel(kind: SignalKind, cancellation: &CancellationToken) -> anyhow::Result<Signal> {
    match signal(kind) {
        Ok(signal) => Ok(signal),
        Err(err) => {
            tracing::error!(?err, "unable to listen for SIGTERM");
            cancellation.cancel();
            Err(anyhow!("signal handler init failed"))
        }
    }
}

pub fn signal_handler() -> CancellationToken {
    let cancellation = CancellationToken::new();
    let task_cancellation = cancellation.clone();
    spawn_named("sys::signal_handler", async move {
        let Ok(mut sigterm) = signal_or_cancel(SignalKind::terminate(), &task_cancellation) else {
            return;
        };

        let Ok(mut sigint) = signal_or_cancel(SignalKind::interrupt(), &task_cancellation) else {
            return;
        };

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
