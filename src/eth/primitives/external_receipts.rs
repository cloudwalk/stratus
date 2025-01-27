use std::collections::HashMap;

use anyhow::anyhow;

use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::Hash;

/// A collection of [`ExternalReceipt`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExternalReceipts(HashMap<Hash, ExternalReceipt>);

impl ExternalReceipts {
    /// Tries to remove a receipt by its hash.
    pub fn try_remove(&mut self, tx_hash: Hash) -> anyhow::Result<ExternalReceipt> {
        match self.0.remove(&tx_hash) {
            Some(receipt) => Ok(receipt),
            None => {
                tracing::error!(%tx_hash, "receipt is missing for hash");
                Err(anyhow!("receipt missing for hash {}", tx_hash))
            }
        }
    }
}

impl From<Vec<ExternalReceipt>> for ExternalReceipts {
    fn from(receipts: Vec<ExternalReceipt>) -> Self {
        let mut receipts_by_hash = HashMap::with_capacity(receipts.len());
        for receipt in receipts {
            receipts_by_hash.insert(receipt.hash(), receipt);
        }
        Self(receipts_by_hash)
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
