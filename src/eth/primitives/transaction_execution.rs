use std::collections::HashMap;
use std::str::FromStr;

use display_json::DebugAsJson;
use ethereum_types::H160;
use ethereum_types::H256;
use ethereum_types::U64;

use super::Gas;
use crate::eth::consensus::append_entry;
use crate::eth::consensus::utils::*;
use crate::eth::evm::EvmExecutionResult;
use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ChainId;
use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::ExecutionMetrics;
use crate::eth::primitives::ExecutionResult;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::ExternalTransaction;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Log;
use crate::eth::primitives::LogTopic;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::UnixTime;
use crate::eth::primitives::Wei;

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
                from: input.from.to_fixed_bytes().to_vec(),
                to: input.to.map(|to| to.to_fixed_bytes().to_vec()),
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
                signer: input.signer.to_fixed_bytes().to_vec(),
                tx_type: input.tx_type.map(|t| t.as_u64()),
                block_timestamp: result.execution.block_timestamp.as_u64(),
            },
        }
    }

    pub fn from_append_entry_transaction(entry: append_entry::TransactionExecutionEntry) -> anyhow::Result<Self> {
        let execution_result = ExecutionResult::from_str(&entry.result).map_err(|_| anyhow::anyhow!("Invalid execution result: {}", entry.result))?;

        let input = TransactionInput {
            tx_type: entry.tx_type.map(U64::from),
            chain_id: entry.chain_id.map(ChainId::from),
            hash: Hash::new_from_h256(H256::from_slice(&entry.hash)),
            nonce: Nonce::from(entry.nonce),
            signer: Address::new_from_h160(H160::from_slice(&entry.signer)),
            from: Address::new_from_h160(H160::from_slice(&entry.from)),
            to: entry.to.map(|to| Address::new_from_h160(H160::from_slice(&to))),
            value: Wei::new(bytes_to_u256(&entry.value)?),
            input: Bytes(entry.input),
            gas_limit: Gas::try_from(bytes_to_u256(&entry.gas_limit)?)?,
            gas_price: Wei::new(bytes_to_u256(&entry.gas_price)?),
            v: U64::from(entry.v),
            r: bytes_to_u256(&entry.r)?,
            s: bytes_to_u256(&entry.s)?,
        };

        let result = EvmExecutionResult {
            execution: EvmExecution {
                block_timestamp: UnixTime::from(entry.block_timestamp),
                receipt_applied: false,
                result: execution_result,
                output: Bytes(entry.output),
                logs: entry
                    .logs
                    .iter()
                    .map(|log| Log {
                        address: Address::new_from_h160(H160::from_slice(&log.address)),
                        topic0: log
                            .topics
                            .first()
                            .and_then(|t| if t.is_empty() { None } else { Some(LogTopic::new(H256::from_slice(t))) }),
                        topic1: log
                            .topics
                            .get(1)
                            .and_then(|t| if t.is_empty() { None } else { Some(LogTopic::new(H256::from_slice(t))) }),
                        topic2: log
                            .topics
                            .get(2)
                            .and_then(|t| if t.is_empty() { None } else { Some(LogTopic::new(H256::from_slice(t))) }),
                        topic3: log
                            .topics
                            .get(3)
                            .and_then(|t| if t.is_empty() { None } else { Some(LogTopic::new(H256::from_slice(t))) }),
                        data: Bytes(log.data.clone()),
                    })
                    .collect(),
                gas: Gas::try_from(bytes_to_u256(&entry.gas)?)?,
                changes: HashMap::new(), // assuming empty for now
                deployed_contract_address: entry.deployed_contract_address.map(|addr| Address::new_from_h160(H160::from_slice(&addr))),
            },
            metrics: ExecutionMetrics::default(), // assuming default metrics for now
        };

        Ok(Self::Local(LocalTransactionExecution { input, result }))
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
