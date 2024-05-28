use std::time::Duration;

use tokio::select;
use tokio::signal::unix::signal;
use tokio::signal::unix::SignalKind;
use uuid::Uuid;

use crate::ext::spawn_named;
use crate::log_and_err;
use crate::GlobalState;

pub fn new_context_id() -> String {
    Uuid::new_v4().to_string()
}

pub async fn spawn_signal_handler() -> anyhow::Result<()> {
    const TASK_NAME: &str = "signal-handler";

    let mut sigterm = match signal(SignalKind::terminate()) {
        Ok(signal) => signal,
        Err(e) => return log_and_err!(reason = e, "failed to init SIGTERM watcher"),
    };
    let mut sigint = match signal(SignalKind::interrupt()) {
        Ok(signal) => signal,
        Err(e) => return log_and_err!(reason = e, "failed to init SIGINT watcher"),
    };

    spawn_named("sys::signal_handler", async move {
        select! {
            _ = sigterm.recv() => {
                GlobalState::shutdown_from(TASK_NAME, "received SIGTERM");
            }
            _ = sigint.recv() => {
                GlobalState::shutdown_from(TASK_NAME, "received SIGINT");
            }
        }
    });

    Ok(())
}

pub fn calculate_tps_and_bpm(duration: Duration, transaction_count: usize, block_count: usize) -> (f64, f64) {
    let seconds_elapsed = duration.as_secs_f64() + f64::EPSILON;
    let tps = transaction_count as f64 / seconds_elapsed;
    let blocks_per_minute = block_count as f64 / (seconds_elapsed / 60.0);
    (tps, blocks_per_minute)
}

pub fn calculate_tps(duration: Duration, transaction_count: usize) -> f64 {
    let seconds_elapsed = duration.as_secs_f64() + f64::EPSILON;
    transaction_count as f64 / seconds_elapsed
}
