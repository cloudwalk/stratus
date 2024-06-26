use display_json::DebugAsJson;

use crate::eth::consensus::append_entry;
use crate::eth::evm::EvmExecutionResult;
use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::ExternalTransaction;
use crate::eth::primitives::Hash;
use crate::eth::primitives::TransactionInput;

#[allow(clippy::large_enum_variant)]
#[derive(DebugAsJson, Clone, strum::EnumIs, serde::Serialize)]
pub enum TransactionExecution {
    /// Transaction that was sent directly to Stratus.
    Local(LocalTransactionExecution),

    /// Transaction that imported from external source.
    External(ExternalTransactionExecution),
}

impl TransactionExecution {
    /// Creates a new transaction execution from a local transaction.
    pub fn new_local(tx: TransactionInput, result: EvmExecutionResult) -> Self {
        Self::Local(LocalTransactionExecution { input: tx, result })
    }

    /// Checks if the current transaction was completed normally.
    pub fn is_success(&self) -> bool {
        match self {
            Self::Local(inner) => inner.is_success(),
            Self::External(inner) => inner.is_success(),
        }
    }

    /// Checks if the current transaction was completed with a failure (reverted or halted).
    pub fn is_failure(&self) -> bool {
        match self {
            Self::Local(inner) => inner.is_failure(),
            Self::External(inner) => inner.is_failure(),
        }
    }

    /// Returns the transaction hash.
    pub fn hash(&self) -> Hash {
        match self {
            Self::Local(LocalTransactionExecution { input, .. }) => input.hash,
            Self::External(ExternalTransactionExecution { tx, .. }) => tx.hash(),
        }
    }

    /// Returns the execution result.
    pub fn result(&self) -> &EvmExecutionResult {
        match self {
            Self::Local(LocalTransactionExecution { result, .. }) => result,
            Self::External(ExternalTransactionExecution { result, .. }) => result,
        }
    }

    /// Returns the execution.
    pub fn execution(&self) -> &EvmExecution {
        match self {
            Self::Local(LocalTransactionExecution { result, .. }) => &result.execution,
            Self::External(ExternalTransactionExecution { result, .. }) => &result.execution,
        }
    }

    /// TODO: use From or TryFrom trait instead of this function
    pub fn to_append_entry_transaction(&self) -> append_entry::TransactionExecutionEntry {
        fn u256_to_bytes(u: ethereum_types::U256) -> Vec<u8> {
            let mut bytes = [0u8; 32];
            u.to_big_endian(&mut bytes);
            bytes.to_vec()
        }

        match self {
            Self::External(ExternalTransactionExecution { tx, receipt, result }) => append_entry::TransactionExecutionEntry {
                hash: tx.hash.to_string(),
                nonce: tx.nonce.as_u64(),
                value: u256_to_bytes(tx.value),
                gas_price: tx.gas_price.map_or(vec![], u256_to_bytes),
                input: tx.input.to_vec(),
                v: tx.v.as_u64(),
                r: u256_to_bytes(tx.r),
                s: u256_to_bytes(tx.s),
                chain_id: tx.chain_id.unwrap_or_default().as_u64(),
                result: result.execution.output.to_vec(),
                output: result.execution.output.to_vec(),
                from: tx.from.to_string(),
                to: tx.to.unwrap_or_default().to_string(),
                block_hash: receipt.block_hash().to_string(),
                block_number: receipt.block_number().as_u64(),
                transaction_index: receipt.transaction_index.as_u64(),
                logs: receipt
                    .logs
                    .iter()
                    .map(|log| append_entry::Log {
                        address: log.address.to_string(),
                        topics: log.topics.iter().map(|topic| topic.into()).collect(),
                        data: log.data.to_vec(),
                        log_index: log.log_index.unwrap_or_default().as_u64(),
                    })
                    .collect(),
                gas: u256_to_bytes(tx.gas),
                receipt_cumulative_gas_used: u256_to_bytes(receipt.cumulative_gas_used),
                receipt_gas_used: receipt.gas_used.map_or(vec![], u256_to_bytes),
                receipt_contract_address: receipt.contract_address.map_or(vec![], |addr| addr.as_bytes().to_vec()),
                receipt_status: receipt.status.unwrap_or_default().as_u32(),
                receipt_logs_bloom: receipt.logs_bloom.as_bytes().to_vec(),
                receipt_effective_gas_price: receipt.effective_gas_price.map_or(vec![], u256_to_bytes),
            },
            // TODO: no need to panic here, this could be implemented
            _ => panic!("Only ExternalTransactionExecution is supported"),
        }
    }
}

#[derive(DebugAsJson, Clone, derive_new::new, serde::Serialize)]
pub struct LocalTransactionExecution {
    pub input: TransactionInput,
    pub result: EvmExecutionResult,
}

impl LocalTransactionExecution {
    /// Check if the current transaction was completed normally.
    pub fn is_success(&self) -> bool {
        self.result.is_success()
    }

    /// Checks if the current transaction was completed with a failure (reverted or halted).
    pub fn is_failure(&self) -> bool {
        self.result.is_failure()
    }
}

#[derive(DebugAsJson, Clone, derive_new::new, serde::Serialize)]
pub struct ExternalTransactionExecution {
    pub tx: ExternalTransaction,
    pub receipt: ExternalReceipt,
    pub result: EvmExecutionResult,
}

impl ExternalTransactionExecution {
    /// Check if the current transaction was completed normally.
    pub fn is_success(&self) -> bool {
        self.result.is_success()
    }

    /// Checks if the current transaction was completed with a failure (reverted or halted).
    pub fn is_failure(&self) -> bool {
        self.result.is_failure()
    }
}
