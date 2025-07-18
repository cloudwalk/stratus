use alloy_consensus::Eip658Value;
use alloy_consensus::Receipt;
use alloy_consensus::ReceiptEnvelope;
use alloy_consensus::ReceiptWithBloom;
use display_json::DebugAsJson;
use itertools::Itertools;

use crate::alias::AlloyReceipt;
use crate::alias::AlloyTransaction;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Index;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::logs_bloom::LogsBloom;
use crate::ext::OptionExt;
use crate::ext::RuintExt;

/// Transaction that was executed by the EVM and added to a block.
#[derive(DebugAsJson, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct TransactionMined {
    /// Transaction input received through RPC.
    pub input: TransactionInput,

    /// Transaction EVM execution result.
    pub execution: EvmExecution,

    /// TODO: either remove logs from EvmExecution or remove them here
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

impl From<TransactionMined> for AlloyTransaction {
    fn from(value: TransactionMined) -> Self {
        let gas_price = value.input.gas_price;
        let tx = AlloyTransaction::from(value.input);

        Self {
            inner: tx.inner,
            block_hash: Some(value.block_hash.into()),
            block_number: Some(value.block_number.as_u64()),
            transaction_index: Some(value.transaction_index.into()),
            effective_gas_price: Some(gas_price),
        }
    }
}

impl From<TransactionMined> for AlloyReceipt {
    fn from(value: TransactionMined) -> Self {
        let receipt = Receipt {
            status: Eip658Value::Eip658(value.is_success()),
            cumulative_gas_used: value.execution.gas.into(), // TODO: implement cumulative gas used correctly
            logs: value.logs.clone().into_iter().map_into().collect(),
        };

        let receipt_with_bloom = ReceiptWithBloom {
            receipt,
            logs_bloom: value.compute_bloom().into(),
        };

        let inner = match value.input.tx_type.map(|tx| tx.as_u64()) {
            Some(1) => ReceiptEnvelope::Eip2930(receipt_with_bloom),
            Some(2) => ReceiptEnvelope::Eip1559(receipt_with_bloom),
            Some(3) => ReceiptEnvelope::Eip4844(receipt_with_bloom),
            Some(4) => ReceiptEnvelope::Eip7702(receipt_with_bloom),
            _ => ReceiptEnvelope::Legacy(receipt_with_bloom),
        };

        Self {
            inner,
            transaction_hash: value.input.hash.into(),
            transaction_index: Some(value.transaction_index.into()),
            block_hash: Some(value.block_hash.into()),
            block_number: Some(value.block_number.as_u64()),
            gas_used: value.execution.gas.into(),
            effective_gas_price: value.input.gas_price,
            blob_gas_used: None,
            blob_gas_price: None,
            from: value.input.signer.into(),
            to: value.input.to.map_into(),
            contract_address: value.execution.contract_address().map_into(),
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
        let mut rng = rand::rng();
        let v = (0..1000)
            .map(|_| create_tx(rng.random_range(0..100), rng.random_range(0..100)))
            .sorted()
            .map(|tx| (tx.block_number.as_u64(), tx.transaction_index.0))
            .collect_vec();
        assert!(is_sorted(&v));
    }
}
