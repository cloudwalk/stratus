use std::time::Duration;

use tokio::time::Instant;
use uuid::Uuid;

/// Amount of bytes in one GB
pub const GIGABYTE: usize = 1024 * 1024 * 1024;

pub fn new_context_id() -> String {
    Uuid::new_v4().to_string()
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

pub struct DropTimer {
    instant: Instant,
    scope_name: &'static str,
}

impl DropTimer {
    pub fn start(scope_name: &'static str) -> Self {
        Self {
            instant: Instant::now(),
            scope_name,
        }
    }
}

impl Drop for DropTimer {
    fn drop(&mut self) {
        tracing::info!("Timer: '{}' ran for `{:?}`", self.scope_name, self.instant.elapsed());
    }
}
