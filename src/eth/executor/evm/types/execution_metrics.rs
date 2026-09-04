use std::ops::Add;
use std::ops::AddAssign;
use std::time::Duration;

#[cfg(feature = "metrics")]
use crate::eth::codegen;
#[cfg(feature = "metrics")]
use crate::eth::codegen::ContractName;
#[cfg(feature = "metrics")]
use crate::eth::codegen::SoliditySignature;
use crate::eth::storage::FoundAt;
use crate::eth::types::Address;
use crate::eth::types::Bytes;
#[cfg(feature = "metrics")]
use crate::eth::types::ExecutionKind;
use crate::eth::types::Gas;
use crate::infra::metrics;

#[derive(Debug, Default, Clone, Copy)]
pub struct ReadStats {
    pub count: usize,
    pub total_time: Duration,
    pub max_time: Duration,
}

impl Add for ReadStats {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self {
            count: self.count + rhs.count,
            total_time: self.total_time + rhs.total_time,
            max_time: self.max_time.max(rhs.max_time),
        }
    }
}

impl AddAssign for ReadStats {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
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

#[derive(Debug, Clone, Copy)]
pub struct ExecutionMetricsContext {
    #[cfg(feature = "metrics")]
    kind: ExecutionKind,
    #[cfg(feature = "metrics")]
    contract: ContractName,
    #[cfg(feature = "metrics")]
    function: SoliditySignature,
}

impl ExecutionMetricsContext {
    pub fn new(kind: ExecutionKind, to: &Option<Address>, input: &Bytes) -> Self {
        #[cfg(feature = "metrics")]
        {
            Self {
                kind,
                contract: codegen::contract_name(to),
                function: codegen::function_sig(input),
            }
        }
        #[cfg(not(feature = "metrics"))]
        {
            let _ = (kind, to, input);
            Self {}
        }
    }
}

#[derive(Debug)]
pub struct ExecutionMetrics {
    storage_metrics: StorageMetrics,
    gas_used: Gas,
    context: ExecutionMetricsContext,
}

impl ExecutionMetrics {
    pub fn new(storage_metrics: StorageMetrics, gas_used: Gas, context: ExecutionMetricsContext) -> Self {
        Self {
            storage_metrics,
            gas_used,
            context,
        }
    }

    #[cfg(feature = "metrics")]
    fn publish(&self) {
        let context = &self.context;
        match context.kind {
            ExecutionKind::CallPast(_) | ExecutionKind::CallLatest(_) | ExecutionKind::AccessList => {
                metrics::inc_executor_local_call_account_reads(self.storage_metrics.account_reads.total_count(), context.contract, context.function);
                metrics::inc_executor_local_call_slot_reads(self.storage_metrics.slot_reads.total_count(), context.contract, context.function);
                metrics::inc_executor_local_call_gas(self.gas_used.as_u64() as usize, context.contract, context.function);
            }
            ExecutionKind::Transaction => {
                metrics::inc_executor_local_transaction_account_reads(self.storage_metrics.account_reads.total_count(), context.contract, context.function);
                metrics::inc_executor_local_transaction_slot_reads(self.storage_metrics.slot_reads.total_count(), context.contract, context.function);
                metrics::inc_executor_local_transaction_gas(self.gas_used.as_u64() as usize, context.contract, context.function);
            }
            ExecutionKind::RPC(_) => (),
        }
        self.storage_metrics.publish();
    }
}

impl Drop for ExecutionMetrics {
    fn drop(&mut self) {
        #[cfg(feature = "metrics")]
        self.publish();
    }
}

impl StorageMetrics {
    fn publish(&self) {
        for (found_at, stats) in self.account_reads.iter() {
            if stats.count > 0 {
                metrics::inc_n_executor_account_reads(stats.count as u64, found_at.as_str());
                metrics::inc_n_executor_account_read_time(stats.total_time.as_nanos() as u64, found_at.as_str());
                metrics::inc_executor_account_read_time_max(stats.max_time, found_at.as_str());
            }
        }
        for (found_at, stats) in self.slot_reads.iter() {
            if stats.count > 0 {
                metrics::inc_n_executor_slot_reads(stats.count as u64, found_at.as_str());
                metrics::inc_n_executor_slot_read_time(stats.total_time.as_nanos() as u64, found_at.as_str());
                metrics::inc_executor_slot_read_time_max(stats.max_time, found_at.as_str());
            }
        }
    }
}

impl StorageStats {
    #[inline]
    pub fn record(&mut self, found_at: FoundAt, elapsed: Duration) {
        let stat = ReadStats {
            count: 1,
            total_time: elapsed,
            max_time: elapsed,
        };
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
