use alloy_consensus::constants::EMPTY_OMMER_ROOT_HASH;
use alloy_consensus::constants::EMPTY_ROOT_HASH;
use alloy_primitives::FixedBytes;
use alloy_primitives::B256;
use alloy_primitives::B64;
use alloy_primitives::U256;
use alloy_rpc_types_eth::Block as AlloyBlock;
use alloy_rpc_types_eth::BlockTransactions;
use display_json::DebugAsJson;
use fake::Dummy;
use fake::Fake;
use fake::Faker;
use hex_literal::hex;
use jsonrpsee::SubscriptionMessage;

use crate::alias::AlloyAddress;
use crate::alias::AlloyBlockVoid;
use crate::alias::AlloyBloom;
use crate::alias::AlloyBytes;
use crate::alias::AlloyConsensusHeader;
use crate::alias::AlloyHeader;
use crate::eth::primitives::logs_bloom::LogsBloom;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::Difficulty;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Hash;
use crate::eth::primitives::MinerNonce;
use crate::eth::primitives::Size;
use crate::eth::primitives::UnixTime;
use crate::ext::InfallibleExt;

/// Special hash used in block mining to indicate no uncle blocks.
const HASH_EMPTY_UNCLES: Hash = Hash::new(hex!("1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347"));

/// Special hash used in block mining to indicate no transaction root and no receipts root.
const HASH_EMPTY_TRIE: Hash = Hash::new(hex!("56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421"));

#[derive(DebugAsJson, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BlockHeader {
    pub number: BlockNumber,
    pub hash: Hash,
    pub transactions_root: Hash,
    pub gas_used: Gas,
    pub gas_limit: Gas,
    pub bloom: LogsBloom,
    pub timestamp: UnixTime,
    pub parent_hash: Hash,
    pub author: Address,
    pub extra_data: Bytes,
    pub miner: Address,
    pub difficulty: Difficulty, // is always 0x0
    pub receipts_root: Hash,
    pub uncle_hash: Hash, // is always 0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347
    pub size: Size,
    pub state_root: Hash,
    pub total_difficulty: Difficulty, // is always 0x0
    pub nonce: MinerNonce,            // is always 0x0000000000000000
}

impl BlockHeader {
    /// Creates a new block header with the given number.
    pub fn new(number: BlockNumber, timestamp: UnixTime) -> Self {
        Self {
            number,
            hash: number.hash(),
            transactions_root: HASH_EMPTY_TRIE,
            gas_used: Gas::ZERO,
            gas_limit: Gas::ZERO,
            bloom: LogsBloom::default(),
            timestamp,
            parent_hash: number.prev().map(|n| n.hash()).unwrap_or(Hash::ZERO),
            author: Address::default(),
            extra_data: Bytes::default(),
            miner: Address::default(),
            difficulty: Difficulty::default(),
            receipts_root: HASH_EMPTY_TRIE,
            uncle_hash: HASH_EMPTY_UNCLES,
            size: Size::default(),
            state_root: HASH_EMPTY_TRIE,
            total_difficulty: Difficulty::default(),
            nonce: MinerNonce::default(),
        }
    }
}

impl Dummy<Faker> for BlockHeader {
    fn dummy_with_rng<R: rand::Rng + ?Sized>(faker: &Faker, rng: &mut R) -> Self {
        Self {
            number: faker.fake_with_rng(rng),
            hash: faker.fake_with_rng(rng),
            transactions_root: faker.fake_with_rng(rng),
            gas_used: faker.fake_with_rng(rng),
            bloom: LogsBloom::default(),
            timestamp: faker.fake_with_rng(rng),
            parent_hash: faker.fake_with_rng(rng),
            gas_limit: faker.fake_with_rng(rng),
            author: faker.fake_with_rng(rng),
            extra_data: faker.fake_with_rng(rng),
            miner: faker.fake_with_rng(rng),
            difficulty: faker.fake_with_rng(rng),
            receipts_root: faker.fake_with_rng(rng),
            uncle_hash: faker.fake_with_rng(rng),
            size: faker.fake_with_rng(rng),
            state_root: faker.fake_with_rng(rng),
            total_difficulty: faker.fake_with_rng(rng),
            nonce: faker.fake_with_rng(rng),
        }
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

impl<T> From<BlockHeader> for AlloyBlock<T> {
    fn from(header: BlockHeader) -> Self {
        let inner = AlloyConsensusHeader {
            // block: identifiers
            number: header.number.as_u64(),
            mix_hash: FixedBytes::default(),

            // block: relation with other blocks
            ommers_hash: EMPTY_OMMER_ROOT_HASH,
            parent_hash: B256::from(header.parent_hash),
            parent_beacon_block_root: None,

            // mining: identifiers
            timestamp: *header.timestamp,
            beneficiary: AlloyAddress::from(header.author),

            // mining: difficulty
            difficulty: U256::ZERO,
            nonce: B64::ZERO,

            // mining: gas
            gas_limit: header.gas_limit.as_u64(),
            gas_used: header.gas_used.as_u64(),
            base_fee_per_gas: Some(0u64),
            blob_gas_used: None,
            excess_blob_gas: None,

            // transactions
            transactions_root: B256::from(header.transactions_root),
            receipts_root: EMPTY_ROOT_HASH,
            withdrawals_root: None,

            // data
            logs_bloom: AlloyBloom::from(header.bloom),
            extra_data: AlloyBytes::default(),
            state_root: B256::from(header.state_root),
            requests_hash: None,
        };

        let rpc_header = AlloyHeader {
            hash: header.hash.into(),
            inner,
            total_difficulty: Some(U256::ZERO),
            size: Some(header.size.into()),
        };

        Self {
            header: rpc_header,
            uncles: Vec::new(),
            transactions: BlockTransactions::default(),
            withdrawals: None,
        }
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl TryFrom<&ExternalBlock> for BlockHeader {
    type Error = anyhow::Error;
    fn try_from(value: &ExternalBlock) -> Result<Self, Self::Error> {
        Ok(Self {
            number: BlockNumber::from(value.0.header.inner.number),
            hash: Hash::from(value.0.header.hash),
            transactions_root: Hash::from(value.0.header.inner.transactions_root),
            gas_used: Gas::from(value.0.header.inner.gas_used),
            gas_limit: Gas::from(value.0.header.inner.gas_limit),
            bloom: LogsBloom::from(value.0.header.inner.logs_bloom),
            timestamp: UnixTime::from(value.0.header.inner.timestamp),
            parent_hash: Hash::from(value.0.header.inner.parent_hash),
            author: Address::from(value.0.header.inner.beneficiary),
            extra_data: Bytes::from(value.0.header.inner.extra_data.clone()),
            miner: Address::from(value.0.header.inner.beneficiary),
            difficulty: Difficulty::from(value.0.header.inner.difficulty),
            receipts_root: Hash::from(value.0.header.inner.receipts_root),
            uncle_hash: Hash::from(value.0.header.inner.ommers_hash),
            size: Size::try_from(value.0.header.size.unwrap_or_default())?,
            state_root: Hash::from(value.0.header.inner.state_root),
            total_difficulty: Difficulty::from(value.0.header.total_difficulty.unwrap_or_default()),
            nonce: MinerNonce::from(value.0.header.inner.nonce.0),
        })
    }
}

impl From<BlockHeader> for SubscriptionMessage {
    fn from(value: BlockHeader) -> Self {
        serde_json::value::RawValue::from_string(serde_json::to_string(&AlloyBlockVoid::from(value)).expect_infallible())
            .expect_infallible()
            .into()
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use crate::eth::primitives::BlockHeader;
    use crate::eth::primitives::BlockNumber;
    use crate::eth::primitives::Hash;
    use crate::eth::primitives::UnixTime;

    #[test]
    fn block_header_hash_calculation() {
        let header = BlockHeader::new(BlockNumber::ZERO, UnixTime::from(1234567890));
        assert_eq!(header.hash.to_string(), "0x011b4d03dd8c01f1049143cf9c4c817e4b167f1d1b83e5c6f0f10d89ba1e7bce");
    }

    #[test]
    fn block_header_parent_hash() {
        let header = BlockHeader::new(BlockNumber::ONE, UnixTime::from(1234567891));
        assert_eq!(
            header.parent_hash.to_string(),
            "0x011b4d03dd8c01f1049143cf9c4c817e4b167f1d1b83e5c6f0f10d89ba1e7bce"
        );
    }

    #[test]
    fn block_header_genesis_parent_hash() {
        let header = BlockHeader::new(BlockNumber::ZERO, UnixTime::from(1234567890));
        assert_eq!(header.parent_hash, Hash::ZERO);
    }
}
