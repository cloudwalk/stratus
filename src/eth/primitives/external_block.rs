use ethers_core::types::Block as EthersBlock;
use ethers_core::types::Transaction as EthersTransaction;

use super::BlockNumber;
use crate::eth::primitives::ExternalTransaction;
use crate::log_and_err;

use super::Block;

#[derive(Debug, Clone, derive_more:: Deref, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct ExternalBlock(#[deref] pub EthersBlock<ExternalTransaction>);

impl ExternalBlock {
    /// Returns the block number.
    pub fn number(&self) -> BlockNumber {
        self.0.number.expect("external block must have number").into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

impl From<ExternalBlock> for EthersBlock<ExternalTransaction> {
    fn from(value: ExternalBlock) -> Self {
        value.0
    }
}

impl From<ExternalBlock> for Block {
    fn from(value: ExternalBlock) -> Self {
        Block {
            header: super::BlockHeader {
                number: value.number.unwrap().into(),
                hash: value.hash.unwrap().into(),
                transactions_root: value.transactions_root.into(),
                gas: value.gas_used.into(),
                bloom: value.logs_bloom.unwrap().into(),
                timestamp_in_secs: value.timestamp.as_u64().into(),
                parent_hash: value.parent_hash.into(),
            },
            transactions: vec![]


// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
impl TryFrom<serde_json::Value> for ExternalBlock {
    type Error = anyhow::Error;

    fn try_from(value: serde_json::Value) -> Result<Self, Self::Error> {
        match serde_json::from_value(value.clone()) {
            Ok(v) => Ok(v),
            Err(e) => log_and_err!(reason = e, payload = value, "failed to convert payload value to ExternalBlock"),
        }
    }
}
          
impl From<EthersBlock<EthersTransaction>> for ExternalBlock {
    fn from(value: EthersBlock<EthersTransaction>) -> Self {
        let txs: Vec<ExternalTransaction> = value.transactions.into_iter().map(ExternalTransaction::from).collect();

        // Is there a better way to do this?
        let block = EthersBlock {
            transactions: txs,
            hash: value.hash,
            parent_hash: value.parent_hash,
            uncles_hash: value.uncles_hash,
            author: value.author,
            state_root: value.state_root,
            transactions_root: value.transactions_root,
            receipts_root: value.receipts_root,
            number: value.number,
            gas_used: value.gas_used,
            gas_limit: value.gas_limit,
            extra_data: value.extra_data,
            logs_bloom: value.logs_bloom,
            timestamp: value.timestamp,
            difficulty: value.difficulty,
            total_difficulty: value.total_difficulty,
            seal_fields: value.seal_fields,
            uncles: value.uncles,
            size: value.size,
            mix_hash: value.mix_hash,
            nonce: value.nonce,
            base_fee_per_gas: value.base_fee_per_gas,
            blob_gas_used: value.blob_gas_used,
            excess_blob_gas: value.excess_blob_gas,
            withdrawals: value.withdrawals,
            withdrawals_root: value.withdrawals_root,
            parent_beacon_block_root: value.parent_beacon_block_root,
            other: value.other,
        };
        ExternalBlock(block)
    }
}
