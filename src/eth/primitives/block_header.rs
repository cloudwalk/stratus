use ethereum_types::Bloom;
use ethereum_types::H64;
use ethereum_types::U256;
use ethers_core::types::Block as EthersBlock;
use fake::Dummy;
use fake::Fake;
use fake::Faker;
use hex_literal::hex;
use jsonrpsee::SubscriptionMessage;

use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Hash;

/// Special hash used in block mining to indicate no uncle blocks.
const HASH_EMPTY_UNCLES: Hash = Hash::new(hex!("1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347"));

/// Special hash used in block mining to indicate no transaction root and no receipts root.
const HASH_EMPTY_TRANSACTIONS_ROOT: Hash = Hash::new(hex!("56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421"));

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BlockHeader {
    pub number: BlockNumber,
    pub hash: Hash,
    pub transactions_root: Hash,
    pub gas: Gas,
    pub bloom: LogsBloom,
    pub timestamp_in_secs: u64,
}

impl BlockHeader {
    /// Creates a new block header with the given number.
    pub fn new(number: BlockNumber, timestamp_in_secs: u64) -> Self {
        Self {
            number,
            hash: Hash::new_random(),
            transactions_root: HASH_EMPTY_TRANSACTIONS_ROOT,
            gas: Gas::ZERO,
            bloom: Bloom::default(),
            timestamp_in_secs,
        }
    }
}

impl Dummy<Faker> for BlockHeader {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(faker: &Faker, rng: &mut R) -> Self {
        Self {
            number: faker.fake_with_rng(rng),
            hash: faker.fake_with_rng(rng),
            transactions_root: faker.fake_with_rng(rng),
            gas: faker.fake_with_rng(rng),
            bloom: Default::default(),
            timestamp_in_secs: faker.fake_with_rng(rng),
        }
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl<T> From<BlockHeader> for EthersBlock<T>
where
    T: Default,
{
    fn from(header: BlockHeader) -> Self {
        Self {
            // block: identifiers
            hash: Some(header.hash.into()),
            number: Some(header.number.into()),

            // block: relation with other blocks
            uncles_hash: HASH_EMPTY_UNCLES.into(),
            uncles: Vec::new(),
            parent_beacon_block_root: None,

            // mining: identifiers
            timestamp: header.timestamp_in_secs.into(),
            author: Some(Address::COINBASE.into()),

            // minining: difficulty
            difficulty: U256::zero(),
            total_difficulty: Some(U256::zero()),
            nonce: Some(H64::zero()),

            // mining: gas
            gas_limit: Gas::from(100_000_000).into(),
            gas_used: header.gas.into(),
            base_fee_per_gas: Some(U256::zero()),
            blob_gas_used: None,
            excess_blob_gas: None,

            // transactions
            transactions_root: header.transactions_root.into(),
            receipts_root: HASH_EMPTY_TRANSACTIONS_ROOT.into(),

            // data
            logs_bloom: Some(header.bloom),
            extra_data: Default::default(),

            // TODO
            ..Default::default() // parent_hash: todo!(),
                                 // state_root: todo!(),

                                 // seal_fields: todo!(),
                                 // transactions: todo!(),
                                 // size: todo!(),
                                 // mix_hash: todo!(),
                                 // withdrawals_root: todo!(),
                                 // withdrawals: todo!(),
                                 // other: todo!(),
        }
    }
}

impl From<BlockHeader> for SubscriptionMessage {
    fn from(value: BlockHeader) -> Self {
        Self::from_json(&value).unwrap()
    }
}
