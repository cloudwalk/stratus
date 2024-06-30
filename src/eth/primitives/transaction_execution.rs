use display_json::DebugAsJson;

use crate::eth::consensus::append_entry;
use crate::eth::consensus::utils::*;
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
        match self {
            Self::External(_) => panic!("Only LocalTransactionExecution is supported"),
            Self::Local(LocalTransactionExecution { input, result }) => append_entry::TransactionExecutionEntry {
                hash: input.hash.as_fixed_bytes().to_vec(),
                nonce: input.nonce.as_u64(),
                value: u256_to_bytes(*input.value.inner()),
                gas_price: u256_to_bytes(*input.gas_price.inner()),
                input: input.input.to_vec(),
                v: input.v.as_u64(),
                r: u256_to_bytes(input.r),
                s: u256_to_bytes(input.s),
                chain_id: Some(input.chain_id.unwrap_or_default().into()),
                result: result.execution.result.to_string(),
                output: result.execution.output.to_vec(),
                from: input.from.as_bytes().to_vec(),
                to: input.to.map(|to| to.as_bytes().to_vec()),
                logs: result
                    .execution
                    .logs
                    .iter()
                    .map(|log| append_entry::Log {
                        address: log.address.as_bytes().to_vec(),
                        topics: vec![
                            log.topic0.map_or_else(Vec::new, |t| t.inner().as_bytes().to_vec()),
                            log.topic1.map_or_else(Vec::new, |t| t.inner().as_bytes().to_vec()),
                            log.topic2.map_or_else(Vec::new, |t| t.inner().as_bytes().to_vec()),
                            log.topic3.map_or_else(Vec::new, |t| t.inner().as_bytes().to_vec()),
                        ],
                        data: log.data.to_vec(),
                    })
                    .collect(),
                gas: u256_to_bytes(result.execution.gas.into()),
                deployed_contract_address: result.execution.deployed_contract_address.map(|addr| addr.as_bytes().to_vec()),
                gas_limit: u256_to_bytes(input.gas_limit.into()),
                signer: input.signer.as_bytes().to_vec(),
                tx_type: input.tx_type.map(|t| t.as_u64()),
                block_timestamp: result.execution.block_timestamp.as_u64(),
            },
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
