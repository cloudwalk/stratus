use std::time::Duration;

use crate::eth::storage::FoundAt;
use crate::eth::types::Gas;
use crate::infra::metrics;

#[derive(Debug, Default, Clone, Copy, derive_more::Add, derive_more::AddAssign)]
pub struct ReadStats {
    pub count: usize,
    pub total_time: Duration,
}

/// Read stats per location, for one entity kind.
#[derive(Debug, Default, Clone, Copy, derive_more::Add, derive_more::AddAssign)]
pub struct StorageStats {
    pub cache: ReadStats,
    pub temp: ReadStats,
    pub perm_latest: ReadStats,
    pub perm_historical: ReadStats,
}

#[derive(Debug, Default)]
pub struct StorageMetrics {
    pub account_reads: StorageStats,
    pub slot_reads: StorageStats,
}

#[derive(Debug, Default)]
#[must_use]
pub struct EvmExecutionMetrics {
    storage_metrics: StorageMetrics,
    gas_used: Gas,
}

impl EvmExecutionMetrics {
    pub fn new(storage_metrics: StorageMetrics, gas_used: Gas) -> Self {
        Self { storage_metrics, gas_used }
    }

    pub fn publish_local_transaction(self, contract: &str, function: &str) {
        metrics::inc_executor_local_transaction_account_reads(self.storage_metrics.account_reads.total_count(), contract, function);
        metrics::inc_executor_local_transaction_slot_reads(self.storage_metrics.slot_reads.total_count(), contract, function);
        metrics::inc_executor_local_transaction_gas(self.gas_used.as_u64() as usize, contract, function);
        self.storage_metrics.publish();
    }

    pub fn publish_local_call(self, contract: &str, function: &str) {
        metrics::inc_executor_local_call_account_reads(self.storage_metrics.account_reads.total_count(), contract, function);
        metrics::inc_executor_local_call_slot_reads(self.storage_metrics.slot_reads.total_count(), contract, function);
        metrics::inc_executor_local_call_gas(self.gas_used.as_u64() as usize, contract, function);
        self.storage_metrics.publish();
    }

    pub fn publish_storage_metrics(self) {
        self.storage_metrics.publish();
    }
}

impl StorageMetrics {
    fn publish(self) {
        for (found_at, stats) in self.account_reads.iter() {
            if stats.count > 0 {
                metrics::inc_n_executor_account_reads(stats.count as u64, found_at.as_str());
                metrics::inc_n_executor_account_read_time(stats.total_time.as_micros() as u64, found_at.as_str());
            }
        }
        for (found_at, stats) in self.slot_reads.iter() {
            if stats.count > 0 {
                metrics::inc_n_executor_slot_reads(stats.count as u64, found_at.as_str());
                metrics::inc_n_executor_slot_read_time(stats.total_time.as_micros() as u64, found_at.as_str());
            }
        }
    }
}

impl StorageStats {
    #[inline]
    pub fn record(&mut self, found_at: FoundAt, elapsed: Duration) {
        let stat = ReadStats { count: 1, total_time: elapsed };
        let stats = match found_at {
            FoundAt::Cache => &mut self.cache,
            FoundAt::Temp => &mut self.temp,
            FoundAt::PermLatest => &mut self.perm_latest,
            FoundAt::PermHistorical => &mut self.perm_historical,
        };
        *stats += stat;
    }

    pub fn get(&self, found_at: FoundAt) -> ReadStats {
        match found_at {
            FoundAt::Cache => self.cache,
            FoundAt::Temp => self.temp,
            FoundAt::PermLatest => self.perm_latest,
            FoundAt::PermHistorical => self.perm_historical,
        }
    }

    fn total_count(&self) -> usize {
        self.cache.count + self.perm_historical.count + self.perm_latest.count + self.temp.count
    }

    /// Iterates over all read locations with their accumulated stats.
    pub fn iter(&self) -> impl Iterator<Item = (FoundAt, ReadStats)> {
        [
            (FoundAt::Cache, self.cache),
            (FoundAt::Temp, self.temp),
            (FoundAt::PermLatest, self.perm_latest),
            (FoundAt::PermHistorical, self.perm_historical),
        ]
        .into_iter()
    }
}
