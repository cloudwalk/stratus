use serde::Deserialize;

use crate::alias::JsonValue;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalReceipt;
use crate::log_and_err;

#[derive(Debug, Clone, Deserialize)]
pub struct ExternalBlockWithReceipts {
    pub block: ExternalBlock,
    pub receipts: Vec<ExternalReceipt>,
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
