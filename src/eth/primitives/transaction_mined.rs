use display_json::DebugAsJson;
use itertools::Itertools;

use crate::alias::EthersReceipt;
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
use crate::if_else;

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
            transaction_index: receipt.0.transaction_index.unwrap_or_default().into(),
            logs: receipt
                .0
                .inner
                .as_receipt()
                .unwrap()
                .logs
                .clone() // TODO: improve before merging move-to-alloy
                .into_iter()
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

impl From<TransactionMined> for EthersReceipt {
    fn from(value: TransactionMined) -> Self {
        let logs_bloom = value.compute_bloom().into();
        Self {
            // receipt specific
            status: Some(if_else!(value.is_success(), 1, 0).into()),
            contract_address: value.execution.contract_address().map_into(),
            gas_used: Some(value.execution.gas.into()),

            // transaction
            transaction_hash: value.input.hash.into(),
            from: value.input.signer.into(),
            to: value.input.to.map_into(),

            // block
            block_hash: Some(value.block_hash.into()),
            block_number: Some(value.block_number.into()),
            transaction_index: value.transaction_index.into(),

            // logs
            logs: value.logs.into_iter().map_into().collect(),
            logs_bloom, // TODO: save this to the database instead of computing it every time (could also be useful for eth_getLogs)

            // TODO: there are more fields to populate here
            ..Default::default()
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
