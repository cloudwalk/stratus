use alloy_consensus::Signed;
use alloy_consensus::TxEnvelope;
use alloy_consensus::TxLegacy;
use alloy_primitives::Bytes;
use alloy_primitives::Signature;
use alloy_primitives::TxKind;
use anyhow::Context;
use anyhow::Result;
use ethereum_types::U256;
use fake::Dummy;
use fake::Fake;
use fake::Faker;

use crate::alias::AlloyTransaction;
use crate::eth::primitives::signature_component::SignatureComponent;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Wei;

#[derive(Debug, Clone, PartialEq, derive_more::Deref, serde::Serialize)]
#[serde(transparent)]
pub struct ExternalTransaction(#[deref] pub AlloyTransaction);

impl<'de> serde::Deserialize<'de> for ExternalTransaction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;
        use serde_json::Value;

        let mut value = Value::deserialize(deserializer)?;

        if let Value::Object(ref mut map) = value {
            // If v is 0x0 or 0x1, this is a type 2 (EIP-1559) transaction
            if let Some(Value::String(v_value)) = map.get("v") {
                if (v_value == "0x0" || v_value == "0x1") && !map.contains_key("type") {
                    map.insert("type".to_string(), Value::String("0x2".to_string()));
                }
            }

            // Check if this is a type 2 transaction
            if let Some(Value::String(type_value)) = map.get("type") {
                if type_value == "0x2" {
                    // For EIP-1559 transactions, ensure max_fee_per_gas and max_priority_fee_per_gas are present
                    if !map.contains_key("maxFeePerGas") {
                        map.insert("maxFeePerGas".to_string(), Value::String("0x0".to_string()));
                    }
                    if !map.contains_key("maxPriorityFeePerGas") {
                        map.insert("maxPriorityFeePerGas".to_string(), Value::String("0x0".to_string()));
                    }
                    if !map.contains_key("accessList") {
                        map.insert("accessList".to_string(), Value::Array(Vec::new()));
                    }
                }
            }
            // Check if this is a type 1 transaction
            if let Some(Value::String(type_value)) = map.get("type") {
                if type_value == "0x1" {
                    // For EIP-2930 transactions, ensure accessList is present
                    if !map.contains_key("accessList") {
                        map.insert("accessList".to_string(), Value::Array(Vec::new()));
                    }
                }
            }
        }

        // Use the inner type's deserialization
        let transaction = AlloyTransaction::deserialize(value).map_err(D::Error::custom)?;

        Ok(ExternalTransaction(transaction))
    }
}

impl ExternalTransaction {
    /// Returns the block number where the transaction was mined.
    pub fn block_number(&self) -> Result<BlockNumber> {
        Ok(self.0.block_number.context("ExternalTransaction has no block_number")?.into())
    }

    /// Returns the transaction hash.
    pub fn hash(&self) -> Hash {
        Hash::from(*self.0.inner.tx_hash())
    }
}

impl Dummy<Faker> for ExternalTransaction {
    fn dummy_with_rng<R: rand_core::RngCore + ?Sized>(faker: &Faker, rng: &mut R) -> Self {
        let from: Address = faker.fake_with_rng(rng);
        let block_hash: Hash = faker.fake_with_rng(rng);

        let gas_price: Wei = Wei::from(rng.next_u64());
        let value: Wei = Wei::from(rng.next_u64());

        let tx = TxLegacy {
            chain_id: Some(1),
            nonce: rng.next_u64(),
            gas_price: gas_price.into(),
            gas_limit: rng.next_u64(),
            to: TxKind::Call(from.into()),
            value: value.into(),
            input: Bytes::default(),
        };

        let r = U256::from(rng.next_u64());
        let s = U256::from(rng.next_u64());
        let v = rng.next_u64() % 2 == 0;
        let signature = Signature::new(SignatureComponent(r).into(), SignatureComponent(s).into(), v);

        let hash: Hash = faker.fake_with_rng(rng);
        let inner_tx = TxEnvelope::Legacy(Signed::new_unchecked(tx, signature, hash.into()));

        let inner = alloy_rpc_types_eth::Transaction {
            inner: inner_tx,
            block_hash: Some(block_hash.into()),
            block_number: Some(rng.next_u64()),
            transaction_index: Some(rng.next_u64()),
            effective_gas_price: Some(gas_price.as_u128()),
        };

        ExternalTransaction(inner)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
impl From<AlloyTransaction> for ExternalTransaction {
    fn from(value: AlloyTransaction) -> Self {
        ExternalTransaction(value)
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn test_deserialize_type0_transaction() {
        let json = json!({
            "hash": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
            "type": "0x0",
            "from": "0x1234567890123456789012345678901234567890",
            "to": "0x0987654321098765432109876543210987654321",
            "gas": "0x76c0",
            "gasPrice": "0x9184e72a000",
            "nonce": "0x1",
            "value": "0x9184e72a",
            "input": "0x",
            "chainId": "0x1",
            "r": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
            "s": "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
            "v": "0x1b"
        });

        let tx: ExternalTransaction = serde_json::from_value(json).unwrap();

        assert!(matches!(tx.0.inner, TxEnvelope::Legacy(_)));
    }

    #[test]
    fn test_deserialize_type1_transaction_with_missing_access_list() {
        let json = json!({
            "hash": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
            "type": "0x1",
            "from": "0x1234567890123456789012345678901234567890",
            "to": "0x0987654321098765432109876543210987654321",
            "gas": "0x76c0",
            "gasPrice": "0x9184e72a000",
            "nonce": "0x1",
            "value": "0x9184e72a",
            "input": "0x",
            "chainId": "0x1",
            "r": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
            "s": "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
            "v": "0x0"
            // accessList is missing
        });

        let tx: ExternalTransaction = serde_json::from_value(json).unwrap();

        assert!(matches!(tx.0.inner, TxEnvelope::Eip2930(_)));
    }

    #[test]
    fn test_deserialize_type2_transaction_with_missing_fields() {
        let json = json!({
            "hash": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
            "type": "0x2",
            "from": "0x1234567890123456789012345678901234567890",
            "to": "0x0987654321098765432109876543210987654321",
            "gas": "0x76c0",
            "nonce": "0x1",
            "value": "0x9184e72a",
            "input": "0x",
            "chainId": "0x1",
            "r": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
            "s": "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
            "v": "0x1"
            // maxFeePerGas, maxPriorityFeePerGas, and accessList are missing
        });

        let tx: ExternalTransaction = serde_json::from_value(json).unwrap();

        assert!(matches!(tx.0.inner, TxEnvelope::Eip1559(_)));
    }

    #[test]
    fn test_deserialize_type2_inferred_from_v_value() {
        let json = json!({
            "hash": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
            "from": "0x1234567890123456789012345678901234567890",
            "to": "0x0987654321098765432109876543210987654321",
            "gas": "0x76c0",
            "gasPrice": "0x9184e72a000",
            "nonce": "0x1",
            "value": "0x9184e72a",
            "input": "0x",
            "chainId": "0x1",
            "r": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
            "s": "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
            "v": "0x0"
            // type field is missing, but v is 0x0 so it should be inferred as type 2
        });

        let tx: ExternalTransaction = serde_json::from_value(json).unwrap();

        assert!(matches!(tx.0.inner, TxEnvelope::Eip1559(_)));

        // Test with v = 0x1 as well
        let json = json!({
            "hash": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
            "from": "0x1234567890123456789012345678901234567890",
            "to": "0x0987654321098765432109876543210987654321",
            "gas": "0x76c0",
            "gasPrice": "0x9184e72a000",
            "nonce": "0x1",
            "value": "0x9184e72a",
            "input": "0x",
            "chainId": "0x1",
            "r": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
            "s": "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
            "v": "0x1"
            // type field is missing, but v is 0x1 so it should be inferred as type 2
        });

        let tx: ExternalTransaction = serde_json::from_value(json).unwrap();

        assert!(matches!(tx.0.inner, TxEnvelope::Eip1559(_)));
    }
}
