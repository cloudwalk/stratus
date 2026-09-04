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
use crate::eth::types::ExecutionKind;
use crate::eth::types::Gas;
#[cfg(feature = "metrics")]
use crate::infra::metrics;

#[derive(Debug, Default, Clone, Copy)]
pub struct ReadStats {
    pub count: usize,
    pub total_time: Duration,
}

impl Add for ReadStats {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self {
            count: self.count + rhs.count,
            total_time: self.total_time + rhs.total_time,
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

/// Stores execution metrics, when this struct is dropped the metrics are recorded.
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
        self.storage_metrics.publish(context);
        metrics::inc_evm_execution_gas(self.gas_used.as_u64() as usize, context.kind.as_ref(), context.contract, context.function);
    }
}

impl Drop for ExecutionMetrics {
    fn drop(&mut self) {
        #[cfg(feature = "metrics")]
        self.publish();
    }
}

impl StorageMetrics {
    #[cfg(feature = "metrics")]
    fn publish(&self, context: &ExecutionMetricsContext) {
        let execution_kind = context.kind.as_ref();
        for (found_at, stats) in self.account_reads.iter() {
            if stats.count > 0 {
                metrics::inc_n_evm_execution_account_reads(stats.count as u64, execution_kind, found_at.as_str(), context.contract, context.function);
                metrics::inc_n_evm_execution_account_read_time(
                    stats.total_time.as_nanos() as u64,
                    execution_kind,
                    found_at.as_str(),
                    context.contract,
                    context.function,
                );
            }
        }
        for (found_at, stats) in self.slot_reads.iter() {
            if stats.count > 0 {
                metrics::inc_n_evm_execution_slot_reads(stats.count as u64, execution_kind, found_at.as_str(), context.contract, context.function);
                metrics::inc_n_evm_execution_slot_read_time(
                    stats.total_time.as_nanos() as u64,
                    execution_kind,
                    found_at.as_str(),
                    context.contract,
                    context.function,
                );
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
