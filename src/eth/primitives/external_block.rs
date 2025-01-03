use serde::Deserialize;

use crate::alias::EthersBlockEthersTransaction;
use crate::alias::EthersBlockExternalTransaction;
use crate::alias::JsonValue;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ExternalTransaction;
use crate::eth::primitives::Hash;
use crate::eth::primitives::UnixTime;
use crate::log_and_err;

#[derive(Debug, Clone, derive_more::Deref, derive_more::DerefMut, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct ExternalBlock(#[deref] pub EthersBlockExternalTransaction);

impl ExternalBlock {
    /// Returns the block hash.
    #[allow(clippy::expect_used)]
    pub fn hash(&self) -> Hash {
        self.0.hash.expect("external block must have hash").into()
    }

    /// Returns the block number.
    #[allow(clippy::expect_used)]
    pub fn number(&self) -> BlockNumber {
        self.0.number.expect("external block must have number").into()
    }

    /// Returns the block timestamp.
    pub fn timestamp(&self) -> UnixTime {
        self.0.timestamp.into()
    }

    /// Returns the block timestamp.
    pub fn author(&self) -> Address {
        self.0.author.unwrap_or_default().into()
    }

    /// Returns the block timestamp.
    pub fn extra_data(&mut self) -> Bytes {
        std::mem::take(&mut self.0.extra_data).into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

impl From<ExternalBlock> for EthersBlockExternalTransaction {
    fn from(value: ExternalBlock) -> Self {
        value.0
    }
}

impl TryFrom<&ExternalBlock> for Block {
    type Error = anyhow::Error;
    fn try_from(value: &ExternalBlock) -> Result<Self, Self::Error> {
        Ok(Block {
            header: value.try_into()?,
            transactions: vec![],
        })
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
impl TryFrom<JsonValue> for ExternalBlock {
    type Error = anyhow::Error;

    fn try_from(value: JsonValue) -> Result<Self, Self::Error> {
        match ExternalBlock::deserialize(&value) {
            Ok(v) => Ok(v),
            Err(e) => log_and_err!(reason = e, payload = value, "failed to convert payload value to ExternalBlock"),
        }
    }
}

impl From<EthersBlockEthersTransaction> for ExternalBlock {
    fn from(value: EthersBlockEthersTransaction) -> Self {
        let txs: Vec<ExternalTransaction> = value.transactions.into_iter().map(ExternalTransaction::from).collect();

        // Is there a better way to do this?
        let block = EthersBlockExternalTransaction {
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
