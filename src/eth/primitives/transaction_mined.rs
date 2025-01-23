use anyhow::anyhow;
use display_json::DebugAsJson;
use itertools::Itertools;
use revm::primitives::alloy_primitives;

use crate::alias::AlloyReceipt;
use crate::alias::EthersTransaction;
use crate::eth::primitives::logs_bloom::LogsBloom;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::ExternalTransaction;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Index;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::TransactionInput;
use crate::ext::OptionExt;

/// Transaction that was executed by the EVM and added to a block.
#[derive(DebugAsJson, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct TransactionMined {
    /// Transaction input received through RPC.
    pub input: TransactionInput,

    /// Transaction EVM execution result.
    pub execution: EvmExecution,

    /// Logs added to the block.
    pub logs: Vec<LogMined>,

    /// Position of the transaction inside the block.
    pub transaction_index: Index,

    /// Block number where the transaction was mined.
    pub block_number: BlockNumber,

    /// Block hash where the transaction was mined.
    pub block_hash: Hash,
}

impl PartialOrd for TransactionMined {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TransactionMined {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.block_number, self.transaction_index).cmp(&(other.block_number, other.transaction_index))
    }
}

impl TransactionMined {
    /// Creates a new mined transaction from an external mined transaction that was re-executed locally.
    ///
    /// TODO: this kind of conversion should be infallibe.
    pub fn from_external(tx: ExternalTransaction, receipt: ExternalReceipt, execution: EvmExecution) -> anyhow::Result<Self> {
        Ok(Self {
            input: tx.clone().try_into()?,
            execution,
            block_number: receipt.block_number(),
            block_hash: receipt.block_hash(),
            transaction_index: receipt
                .0
                .transaction_index
                .map_into()
                .ok_or_else(|| anyhow!("external receipt missing transaction index"))?,
            logs: receipt
                .0
                .inner
                .logs()
                .iter()
                .cloned()
                .map(LogMined::try_from)
                .collect::<Result<Vec<LogMined>, _>>()?,
        })
    }

    /// Check if the current transaction was completed normally.
    pub fn is_success(&self) -> bool {
        self.execution.is_success()
    }

    fn compute_bloom(&self) -> LogsBloom {
        let mut bloom = LogsBloom::default();
        for log_mined in self.logs.iter() {
            bloom.accrue_log(&(log_mined.log));
        }
        bloom
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<TransactionMined> for EthersTransaction {
    fn from(value: TransactionMined) -> Self {
        let input = value.input;
        Self {
            chain_id: input.chain_id.map_into(),
            hash: input.hash.into(),
            nonce: input.nonce.into(),
            block_hash: Some(value.block_hash.into()),
            block_number: Some(value.block_number.into()),
            transaction_index: Some(value.transaction_index.into()),
            from: input.signer.into(),
            to: input.to.map_into(),
            value: input.value.into(),
            gas_price: Some(input.gas_price.into()),
            gas: input.gas_limit.into(),
            input: input.input.into(),
            v: input.v,
            r: input.r,
            s: input.s,
            transaction_type: input.tx_type,
            ..Default::default()
        }
    }
}

impl From<TransactionMined> for AlloyReceipt {
    // TODO: improve before merging move-to-alloy
    fn from(value: TransactionMined) -> Self {
        let receipt = alloy_consensus::Receipt {
            status: if value.is_success() {
                alloy_consensus::Eip658Value::Eip658(true)
            } else {
                alloy_consensus::Eip658Value::Eip658(false)
            },
            cumulative_gas_used: value.execution.gas.into(),
            logs: value.logs.clone().into_iter().map_into().collect(),
        };

        let receipt_with_bloom = alloy_consensus::ReceiptWithBloom {
            receipt,
            logs_bloom: value.compute_bloom().into(),
        };

        let inner = alloy_consensus::ReceiptEnvelope::Legacy(receipt_with_bloom);

        Self {
            inner,
            transaction_hash: {
                let bytes: [u8; 32] = value.input.hash.0.to_fixed_bytes();
                alloy_primitives::B256::from(bytes)
            },
            transaction_index: Some(value.transaction_index.into()),
            block_hash: Some({
                let bytes: [u8; 32] = value.block_hash.0.to_fixed_bytes();
                alloy_primitives::B256::from(bytes)
            }),
            block_number: Some(value.block_number.as_u64()),
            gas_used: value.execution.gas.into(),
            effective_gas_price: value.input.gas_price.as_u128(),
            blob_gas_used: None,
            blob_gas_price: None,
            from: value.input.signer.into(),
            to: value.input.to.map_into(),
            contract_address: value.execution.contract_address().map_into(),
            authorization_list: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use fake::Fake;
    use fake::Faker;
    use rand::Rng;

    use super::*;

    fn create_tx(transaction_index: u64, block_number: u64) -> TransactionMined {
        TransactionMined {
            input: Faker.fake(),
            execution: Faker.fake(),
            logs: vec![],
            transaction_index: transaction_index.into(),
            block_number: block_number.into(),
            block_hash: Hash::default(),
        }
    }

    fn is_sorted<T: Ord>(vec: &[T]) -> bool {
        vec.windows(2).all(|w| w[0] <= w[1])
    }

    #[test]
    fn sort_transactions() {
        let mut rng = rand::thread_rng();
        let v = (0..1000)
            .map(|_| create_tx(rng.gen_range(0..100), rng.gen_range(0..100)))
            .sorted()
            .map(|tx| (tx.block_number.as_u64(), tx.transaction_index.0))
            .collect_vec();
        assert!(is_sorted(&v));
    }
}
