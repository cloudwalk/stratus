use std::collections::HashMap;

use ethereum_types::H256;
use ethers_core::types::TransactionReceipt;

pub fn parse_receipt(receipt_jsons: Vec<&str>) -> anyhow::Result<HashMap<H256, TransactionReceipt>> {
    let mut receipts_map = HashMap::new();

    for receipt_json in receipt_jsons {
        let receipt: TransactionReceipt = serde_json::from_str(receipt_json).map_err(|e| anyhow::anyhow!("Failed to parse receipt JSON: {:?}", e))?;
        receipts_map.insert(receipt.transaction_hash, receipt);
    }

    Ok(receipts_map)
}

use ethers_core::types::Block as ECBlock;
use ethers_core::types::Transaction as ECTransaction;

pub fn parse_block(block_json: &str) -> anyhow::Result<ECBlock<ECTransaction>> {
    let block: ECBlock<ECTransaction> = serde_json::from_str(block_json)?;
    Ok(block)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use ethers_core::types::H256;
    use ethers_core::types::U64;
    use hex_literal::hex;

    use super::*;

    #[test]
    fn test_parse_receipts() {
        let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("receipt_fixture.json");

        // This JSON string represents a single transaction receipt. For testing, it needs to be wrapped in an array.
        let single_receipt_json = fs::read_to_string(fixture_path).expect("Failed to read fixture file");

        let receipts_map = parse_receipt(vec![&single_receipt_json]).unwrap();

        // Since we are testing with a known good value, unwrap is used directly
        let expected_tx_hash = H256::from_slice(&hex!("5a493d5b9e4f36a7569407988e2f9506334de3a7ce3b451c594ab1496cc5e13f"));
        assert!(
            receipts_map.contains_key(&expected_tx_hash),
            "Receipts map does not contain the expected transaction hash."
        );
    }

    #[test]
    fn test_parse_block() {
        let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("block_fixture.json");

        // This JSON string represents a single transaction receipt. For testing, it needs to be wrapped in an array.
        let block_json = fs::read_to_string(fixture_path).expect("Failed to read fixture file");

        let block = parse_block(&block_json).unwrap();
        assert_eq!(block.number.unwrap(), U64::from(58463146));
        assert_eq!(block.transactions.len(), 1);
    }
}
