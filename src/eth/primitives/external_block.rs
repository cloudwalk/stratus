use alloy_eips::eip4895::Withdrawals;
use alloy_primitives::Bloom;
use alloy_primitives::Bytes;
use alloy_primitives::B256;
use alloy_primitives::B64;
use alloy_primitives::U256;
use fake::Dummy;
use fake::Fake;
use fake::Faker;
use serde::Deserialize;

use super::Block;
use crate::alias::AlloyBlockExternalTransaction;
use crate::alias::JsonValue;
use crate::eth::primitives::external_transaction::ExternalTransaction;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::UnixTime;
use crate::log_and_err;

#[derive(Debug, Clone, PartialEq, derive_more::Deref, derive_more::DerefMut, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct ExternalBlock(#[deref] pub AlloyBlockExternalTransaction);

impl ExternalBlock {
    /// Returns the block hash.
    #[allow(clippy::expect_used)]
    pub fn hash(&self) -> Hash {
        Hash::from(self.0.header.hash)
    }

    /// Returns the block number.
    #[allow(clippy::expect_used)]
    pub fn number(&self) -> BlockNumber {
        BlockNumber::from(self.0.header.inner.number)
    }

    /// Returns the block timestamp.
    pub fn timestamp(&self) -> UnixTime {
        self.0.header.inner.timestamp.into()
    }

    /// Returns the block author.
    pub fn author(&self) -> Address {
        self.0.header.inner.beneficiary.into()
    }
}

impl PartialEq<Block> for ExternalBlock {
    fn eq(&self, other: &Block) -> bool {
        self.number() == other.number() && self.timestamp() == other.header.timestamp && self.hash() == other.header.hash
    }
}

impl Dummy<Faker> for ExternalBlock {
    fn dummy_with_rng<R: rand_core::RngCore + ?Sized>(faker: &Faker, rng: &mut R) -> Self {
        let mut addr_bytes = [0u8; 20];
        let mut hash_bytes = [0u8; 32];
        let mut nonce_bytes = [0u8; 8];
        rng.fill_bytes(&mut addr_bytes);
        rng.fill_bytes(&mut hash_bytes);
        rng.fill_bytes(&mut nonce_bytes);

        let transaction: ExternalTransaction = faker.fake_with_rng(rng);

        let block = alloy_rpc_types_eth::Block {
            header: alloy_rpc_types_eth::Header {
                hash: B256::from_slice(&hash_bytes),
                inner: alloy_consensus::Header {
                    parent_hash: B256::from_slice(&hash_bytes),
                    ommers_hash: B256::from_slice(&hash_bytes),
                    beneficiary: alloy_primitives::Address::from_slice(&addr_bytes),
                    state_root: B256::from_slice(&hash_bytes),
                    transactions_root: B256::from_slice(&hash_bytes),
                    receipts_root: B256::from_slice(&hash_bytes),
                    withdrawals_root: Some(B256::from_slice(&hash_bytes)),
                    number: rng.next_u64(),
                    gas_used: rng.next_u64(),
                    gas_limit: rng.next_u64(),
                    extra_data: Bytes::default(),
                    logs_bloom: Bloom::default(),
                    timestamp: rng.next_u64(),
                    difficulty: U256::from(rng.next_u64()),
                    mix_hash: B256::from_slice(&hash_bytes),
                    nonce: B64::from_slice(&nonce_bytes),
                    base_fee_per_gas: Some(rng.next_u64()),
                    blob_gas_used: None,
                    excess_blob_gas: None,
                    parent_beacon_block_root: None,
                    requests_hash: None,
                },
                total_difficulty: Some(U256::from(rng.next_u64())),
                size: Some(U256::from(rng.next_u64())),
            },
            uncles: vec![B256::from_slice(&hash_bytes)],
            transactions: alloy_rpc_types_eth::BlockTransactions::Full(vec![transaction]),
            withdrawals: Some(Withdrawals::default()),
        };

        ExternalBlock(block)
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
