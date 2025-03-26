use fake::Dummy;
use fake::Faker;
use serde::Deserialize;
use serde::Serialize;

use crate::alias::JsonValue;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalReceipt;
use crate::log_and_err;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ExternalBlockWithReceipts {
    pub block: ExternalBlock,
    pub receipts: Vec<ExternalReceipt>,
}

impl Dummy<Faker> for ExternalBlockWithReceipts {
    fn dummy_with_rng<R: rand_core::RngCore + ?Sized>(faker: &Faker, rng: &mut R) -> Self {
        let block = ExternalBlock::dummy_with_rng(faker, rng);

        let receipts = match &block.transactions {
            alloy_rpc_types_eth::BlockTransactions::Full(txs) => txs.iter().map(|_| ExternalReceipt::dummy_with_rng(faker, rng)).collect(),
            alloy_rpc_types_eth::BlockTransactions::Hashes(_) => Vec::new(),
            alloy_rpc_types_eth::BlockTransactions::Uncle => Vec::new(),
        };

        Self { block, receipts }
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl TryFrom<JsonValue> for ExternalBlockWithReceipts {
    type Error = anyhow::Error;

    fn try_from(value: JsonValue) -> Result<Self, Self::Error> {
        match ExternalBlockWithReceipts::deserialize(&value) {
            Ok(v) => Ok(v),
            Err(e) => log_and_err!(reason = e, payload = value, "failed to convert payload value to ExternalBlockWithReceipts"),
        }
    }
}
