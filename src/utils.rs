use std::time::Duration;

use alloy_primitives::Uint;
use fake::Dummy;
use fake::Fake;
use fake::Faker;
use tokio::time::Instant;

/// Amount of bytes in one GB (technically, GiB).
pub const GIGABYTE: usize = 1024 * 1024 * 1024;

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
        tracing::info!(ran_for = ?self.instant.elapsed(), "Timer: '{}' finished", self.scope_name);
    }
}

fn generate_rng() -> rand::rngs::SmallRng {
    use std::time::SystemTime;
    use std::time::UNIX_EPOCH;

    use rand::SeedableRng;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).expect("Failed to get system time").as_secs();
    rand::rngs::SmallRng::seed_from_u64(now)
}

pub fn fake_option<T: Dummy<Faker>>() -> Option<T> {
    let mut rng = generate_rng();
    Some(Faker.fake_with_rng::<T, _>(&mut rng))
}

pub fn fake_option_uint<const N: usize, const L: usize>() -> Option<Uint<N, L>> {
    let mut rng = generate_rng();
    Some(Uint::random_with(&mut rng))
}

#[cfg(test)]
pub mod test_utils {
    use anyhow::Context;
    use fake::Fake;
    use fake::Faker;
    use glob::glob;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    fn deterministic_rng() -> SmallRng {
        SeedableRng::seed_from_u64(0)
    }

    /// Fake the first `size` values of type `T` using the default seed.
    ///
    /// Multiple calls of this (for the same `T` and `size`) will return the same list.
    pub fn fake_list<T: fake::Dummy<Faker>>(size: usize) -> Vec<T> {
        let mut rng = deterministic_rng();
        (0..size).map(|_| Faker.fake_with_rng::<T, _>(&mut rng)).collect()
    }

    /// Fake the first `T` value in the default seed.
    ///
    /// Multiple calls of this (for the same `T`) will return the same value.
    pub fn fake_first<T: fake::Dummy<Faker>>() -> T {
        let mut rng = deterministic_rng();
        Faker.fake_with_rng::<T, _>(&mut rng)
    }

    pub fn glob_to_string_paths(pattern: impl AsRef<str>) -> anyhow::Result<Vec<String>> {
        let pattern = pattern.as_ref();

        let iter = glob(pattern).with_context(|| format!("failed to parse glob pattern: {pattern}"))?;
        let mut paths = vec![];

        for entry in iter {
            let entry = entry.with_context(|| format!("failed to read glob entry with pattern: {pattern}"))?;
            let path = entry.to_str().with_context(|| format!("failed to convert path to string: {entry:?}"))?;
            paths.push(path.to_owned());
        }

        Ok(paths)
    }
}
