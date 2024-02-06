use std::collections::HashMap;

use ethereum_types::H256;
use ethers_core::types::TransactionReceipt;
use serde_json::Value;

pub fn parse_receipt(receipt_json: &str) -> anyhow::Result<HashMap<H256, TransactionReceipt>> {
    let receipt: TransactionReceipt = serde_json::from_str(receipt_json)
        .map_err(|e| anyhow::anyhow!("Failed to parse receipt JSON: {:?}", e))?;

    let mut receipts_map = HashMap::new();
    receipts_map.insert(receipt.transaction_hash, receipt);

    Ok(receipts_map)
}

use ethers_core::types::{Block as ECBlock, Transaction as ECTransaction};

pub fn parse_block(block_json: &str) -> anyhow::Result<ECBlock<ECTransaction>> {
    let block: ECBlock<ECTransaction> = serde_json::from_str(block_json)?;
    Ok(block)
}

#[cfg(test)]
mod tests {
    use super::*; // Import everything from the outer module
    use ethers_core::{types::{H256, U64}, k256::U256};
    use hex_literal::hex;


    #[test]
    fn test_parse_receipts() {
        // This JSON string represents a single transaction receipt. For testing, it needs to be wrapped in an array.
        let single_receipt_json = r#"
            {
                "transactionHash": "0x5a493d5b9e4f36a7569407988e2f9506334de3a7ce3b451c594ab1496cc5e13f",
                "transactionIndex": "0x5",
                "blockHash": "0x4c66dfac45f38e70b3f4147833bb2a9cd39c6d2bcf14df2e7961af59cb19a0b9",
                "blockNumber": "0x37a4cb3",
                "from": "0xbae210bd5e97315275648ab7e7c341146064c903",
                "to": "0x1f94a163c329bec14c73ca46c66150e3c47dbedc",
                "gasUsed": "0x35cef",
                "cumulativeGasUsed": "0xc7244",
                "contractAddress": null,
                "logs": [],
                "logsBloom": "0x00000000000000000000000000000000000000000000004000000000000000000000000000000080000002000000000000000000000000000000000000200000000001000000000000000008000100000000000000000000000000000000000000000000000000000000000000000000000000000000000000080010002000004000000050000000000000000000000000000000000000000000000000000000000000000000000002000000000000000000000000008000000000000000100000000002000200000000000000140000000000000000100000000000000000000001000000000000000000000000000000000012000000000000000000000000",
                "status": "0x1"
            }
        "#;

        let receipts_map = parse_receipt(single_receipt_json).unwrap();

        // Since we are testing with a known good value, unwrap is used directly
        let expected_tx_hash = H256::from_slice(&hex!("5a493d5b9e4f36a7569407988e2f9506334de3a7ce3b451c594ab1496cc5e13f"));
        assert!(receipts_map.contains_key(&expected_tx_hash), "Receipts map does not contain the expected transaction hash.");
    }


    #[test]
    fn test_parse_block() {
        let block_json = r#"
            {
            "author": "0x7bd89e22065e65fc9e2b8015e1119e53815f4fa9",
            "difficulty": "0x0",
            "extraData": "0x",
            "gasLimit": "0x989680",
            "gasUsed": "0x1a1933",
            "hash": "0x938752fb64673fce23ca299a6c6818669aea3de8889224c8bcf16a507a98b3c0",
            "logsBloom": "0x00400800000000800000000000040800000000000180001001000100800080000000402060020000000003002000000000040040000000008400020600140024040001020000001000000008000100002000000100800000000420820000004000020008060000000082010041002800080000000008000200080010000000004401000851000080000020000000000040000000000000001010000020600000410000220100000402080000000000002002820000000000000000000000000000000002000000100000000200141000010104080000110000440008004020001001000020000004000000000000100800000202100c00000080008010008080",
            "miner": "0x7bd89e22065e65fc9e2b8015e1119e53815f4fa9",
            "nonce": "0x0000000000000000",
            "number": "0x37c13aa",
            "parentHash": "0xa089c1c840b8c58470e5fb790e9f8fbeb12239fab19b22c8d59e2ed510093dbe",
            "receiptsRoot": "0x7814697e943a5fdaabea7dde1555dbf6fbd27a83f9e5b0fda4a4c1a815946dd5",
            "sha3Uncles": "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
            "size": "0xab3",
            "stateRoot": "0xce5f38c7cfd159516b2ae656cace439e7c1ac59884f5f118286e79a98d12dbee",
            "timestamp": "0x65c2c08f",
            "totalDifficulty": "0x0",
            "transactions": [
                {
                "accessList": null,
                "blockHash": "0x938752fb64673fce23ca299a6c6818669aea3de8889224c8bcf16a507a98b3c0",
                "blockNumber": "0x37c13aa",
                "chainId": "0x7d9",
                "creates": null,
                "from": "0x2ea765ad326d24851781fd7d81c4a7193d5dc903",
                "gas": "0x61a80",
                "gasPrice": "0x0",
                "hash": "0x807e0889673025c10dc0f95d35217e59b2f3acf98912a3eaba375fa15e3534d4",
                "input": "0xe3541348000000000000000000000000df76192480b1082a898063e32991fa5108247c860000000000000000000000000000000000000000000000000000000000f4240045303231373334343732303234303230363233323832374c506f4d5879504174",
                "nonce": "0xa73ad",
                "publicKey": "0xf2251d2f56cc48d79a2700d654e6e5763bf8bd6a33b2dc02ad104b0d255326bff6ae31289468adf0c9983b1b91d6d48d5564952b7930151788ac59526c36679a",
                "r": "0x4fe10abce1987e7bcc13d5b90837df22320b1aeb818fdd4d79ecf01b6b1fffbc",
                "raw": "0xf8ca830a73ad8083061a80941f94a163c329bec14c73ca46c66150e3c47dbedc80b864e3541348000000000000000000000000df76192480b1082a898063e32991fa5108247c860000000000000000000000000000000000000000000000000000000000f4240045303231373334343732303234303230363233323832374c506f4d5879504174820fd5a04fe10abce1987e7bcc13d5b90837df22320b1aeb818fdd4d79ecf01b6b1fffbca0036ec7073a863e863bd88301d75a7e19f182108f3624447b2d19fac43f110e0b",
                "s": "0x36ec7073a863e863bd88301d75a7e19f182108f3624447b2d19fac43f110e0b",
                "standardV": "0x0",
                "to": "0x1f94a163c329bec14c73ca46c66150e3c47dbedc",
                "transactionIndex": "0xb",
                "type": "0x0",
                "v": "0xfd5",
                "value": "0x0"
                }
            ],
            "transactionsRoot": "0xa4513dd3b91d6ed0b90c41701df5066dadd53efaeec189b972855a3131fa7b63",
            "uncles": []
            }
        "#;

        let block = parse_block(block_json).unwrap();
        assert_eq!(block.number.unwrap(), U64::from(58463146));
        assert_eq!(block.transactions.len(), 1);
    }
}
