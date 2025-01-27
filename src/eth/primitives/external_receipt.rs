use ethereum_types::U256;
use serde::de::Deserializer;
use serde::de::{self};
use serde::Deserialize;

use crate::alias::AlloyReceipt;
use crate::alias::JsonValue;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Wei;
use crate::log_and_err;

#[derive(Debug, Clone, derive_more::Deref, serde::Serialize)]
#[serde(transparent)]
pub struct ExternalReceipt(#[deref] pub AlloyReceipt);

impl ExternalReceipt {
    /// Returns the transaction hash.
    pub fn hash(&self) -> Hash {
        Hash::from(self.0.transaction_hash.0)
    }

    /// Returns the block number.
    #[allow(clippy::expect_used)]
    pub fn block_number(&self) -> BlockNumber {
        self.0.block_number.expect("external receipt must have block number").into()
    }

    /// Returns the block hash.
    #[allow(clippy::expect_used)]
    pub fn block_hash(&self) -> Hash {
        Hash::from(self.0.block_hash.expect("external receipt must have block hash").0)
    }

    /// Retuns the effective price the sender had to pay to execute the transaction.
    pub fn execution_cost(&self) -> Wei {
        let gas_price = U256::from(self.0.effective_gas_price);
        let gas_used = U256::from(self.0.gas_used);
        (gas_price * gas_used).into()
    }

    /// Checks if the transaction was completed with success.
    pub fn is_success(&self) -> bool {
        self.0.inner.status()
    }
}

// -----------------------------------------------------------------------------
// Serialization / Deserilization
// -----------------------------------------------------------------------------

impl<'de> Deserialize<'de> for ExternalReceipt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // During migration from ethers to alloy, we need to handle receipts from both libraries.
        // Ethers receipts do not include `effectiveGasPrice` and `type` fields which are
        // required by alloy.
        let mut value = serde_json::Value::deserialize(deserializer)?;

        if let Some(obj) = value.as_object_mut() {
            if !obj.contains_key("effectiveGasPrice") {
                obj.insert("effectiveGasPrice".to_string(), serde_json::json!("0x0"));
            }
            if !obj.contains_key("type") {
                obj.insert("type".to_string(), serde_json::json!("0x0"));
            }
        } else {
            return Err(de::Error::custom("ExternalReceipt must be a JSON object, received invalid type"));
        }

        let receipt = serde_json::from_value(value).map_err(|e| de::Error::custom(format!("Failed to deserialize ExternalReceipt: {}", e)))?;

        Ok(ExternalReceipt(receipt))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl TryFrom<JsonValue> for ExternalReceipt {
    type Error = anyhow::Error;

    fn try_from(value: JsonValue) -> Result<Self, Self::Error> {
        match ExternalReceipt::deserialize(&value) {
            Ok(v) => Ok(v),
            Err(e) => log_and_err!(reason = e, payload = value, "failed to convert payload value to ExternalBlock"),
        }
    }
}

#[cfg(test)]
mod tests {
    use alloy_consensus::TxType;

    use super::*;

    #[test]
    fn test_deserialize_old_receipt() {
        let old_receipt = r#"{
            "blockHash": "0xc05ff25c9e4bcfb57a5bab271a38b46a8c8b2d5d9ef815ba449d6e211da42251",
            "blockNumber": "0x20",
            "contractAddress": null,
            "cumulativeGasUsed": "0x0",
            "from": "0x4fe666531f4a27d0cf5e3d2e73d9122a7f03777b",
            "gasUsed": "0xe19c",
            "logs": [{
                "address": "0xe7f1725e7734ce288f8367e1bb143e90bb3f0512",
                "blockHash": "0xc05ff25c9e4bcfb57a5bab271a38b46a8c8b2d5d9ef815ba449d6e211da42251",
                "blockNumber": "0x20",
                "data": "0x000000000000000000000000000000000000000000000000000000000000000a",
                "logIndex": "0x0",
                "removed": false,
                "topics": [
                    "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef",
                    "0x0000000000000000000000004fe666531f4a27d0cf5e3d2e73d9122a7f03777b",
                    "0x000000000000000000000000673dfa23201c98b7a3bfb48fc5cc4011d6759869"
                ],
                "transactionHash": "0x1c9b122e1321398ac869512b121f97c057e28e0e2fa96e9a8df1ecbfa9824faf",
                "transactionIndex": "0x20"
            }],
            "logsBloom": "0x00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010000000000000000000004200000000000000000000000000000008000000000000080000000000000000000000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000800000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000000000002000000000000000000001000000000000000000000000000080000000000000000000000000000000000000000000000000000800000000000000000",
            "status": "0x1",
            "to": "0xe7f1725e7734ce288f8367e1bb143e90bb3f0512",
            "transactionHash": "0x1c9b122e1321398ac869512b121f97c057e28e0e2fa96e9a8df1ecbfa9824faf",
            "transactionIndex": "0x20"
        }"#;

        let receipt: ExternalReceipt = serde_json::from_str(old_receipt).unwrap();
        assert_eq!(receipt.0.effective_gas_price, 0);
        assert_eq!(receipt.0.transaction_type(), TxType::Legacy);
    }

    #[test]
    fn test_deserialize_new_receipt() {
        let new_receipt = r#"{
            "blockHash": "0x20dd72172e4bd9c99a919c217dd8c0154cbe0f9e305e67c5247f2ee8ae987c06",
            "blockNumber": "0x16",
            "contractAddress": null,
            "cumulativeGasUsed": "0xe19c",
            "effectiveGasPrice": "0x0",
            "from": "0x08ea581a1da0e4c8a3e494501102c1cb16a89d1d",
            "gasUsed": "0xe19c",
            "logs": [{
                "address": "0xe7f1725e7734ce288f8367e1bb143e90bb3f0512",
                "blockHash": "0x20dd72172e4bd9c99a919c217dd8c0154cbe0f9e305e67c5247f2ee8ae987c06",
                "blockNumber": "0x16",
                "data": "0x0000000000000000000000000000000000000000000000000000000000000002",
                "logIndex": "0x0",
                "removed": false,
                "topics": [
                    "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef",
                    "0x00000000000000000000000008ea581a1da0e4c8a3e494501102c1cb16a89d1d",
                    "0x0000000000000000000000008259d2809ea92d5fad80c279ea11d2e371b8e33c"
                ],
                "transactionHash": "0x8eef471d6dad6584888af17b80f01f25f79875a0e0a1cbd17809c74093381bbc",
                "transactionIndex": "0x26"
            }],
            "logsBloom": "0x00000000000000000000000000000000000000000000000000000000000000000000000000100000000000000000000010000000000000000000000000010000000000000000000000000008000000000000000000000002000000000000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000000000000000000002000004000000000000001000000000000000000000000000000000000000000000000000000000000000000000000000000000800000000000000000",
            "status": "0x1",
            "to": "0xe7f1725e7734ce288f8367e1bb143e90bb3f0512",
            "transactionHash": "0x8eef471d6dad6584888af17b80f01f25f79875a0e0a1cbd17809c74093381bbc",
            "transactionIndex": "0x26",
            "type": "0x0"
        }"#;

        let receipt: ExternalReceipt = serde_json::from_str(new_receipt).unwrap();
        assert_eq!(receipt.0.effective_gas_price, 0);
        assert_eq!(receipt.0.transaction_type(), TxType::Legacy);
    }
}
