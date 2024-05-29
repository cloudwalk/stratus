use std::time::Duration;

use uuid::Uuid;

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
