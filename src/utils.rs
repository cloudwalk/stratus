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

#[cfg(test)]
pub mod test_utils {
    use fake::Fake;
    use fake::Faker;
    use rand::rngs::SmallRng;
    use rand::SeedableRng;

    fn deterministic_rng() -> &'static mut SmallRng {
        let rng = SeedableRng::seed_from_u64(0);
        Box::leak(Box::new(rng))
    }

    /// Fake the first `size` values of type `T` using the default seed.
    ///
    /// Multiple calls of this (for the same `T` and `size`) will return the same list.
    pub fn fake_list<T: fake::Dummy<Faker>>(size: usize) -> Vec<T> {
        let rng = deterministic_rng();
        (0..size).map(|_| Faker.fake_with_rng::<T, _>(rng)).collect()
    }

    /// Fake the first `T` value in the default seed.
    ///
    /// Multiple calls of this (for the same `T`) will return the same value.
    pub fn fake_first<T: fake::Dummy<Faker>>() -> T {
        let rng = deterministic_rng();
        Faker.fake_with_rng::<T, _>(rng)
    }
}
