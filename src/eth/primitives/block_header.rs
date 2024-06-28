use ethereum_types::Bloom;
use ethereum_types::H160;
use ethereum_types::H256;
use ethereum_types::H64;
use ethereum_types::U256;
use ethers_core::types::Block as EthersBlock;
use fake::Dummy;
use fake::Fake;
use fake::Faker;
use hex_literal::hex;
use jsonrpsee::SubscriptionMessage;

use crate::eth::consensus::append_entry;
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

/// Special hash used in block mining to indicate no uncle blocks.
const HASH_EMPTY_UNCLES: Hash = Hash::new(hex!("1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347"));

/// Special hash used in block mining to indicate no transaction root and no receipts root.
const HASH_EMPTY_TRIE: Hash = Hash::new(hex!("56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421"));

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
            parent_hash: number.prev().map(|n| n.hash()).unwrap_or(Hash::zero()),
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

    pub fn to_append_entry_block_header(&self, transaction_hashes: Vec<Vec<u8>>) -> append_entry::BlockEntry {
        append_entry::BlockEntry {
            number: self.number.into(),
            hash: self.hash.as_fixed_bytes().to_vec(),
            transactions_root: self.transactions_root.as_fixed_bytes().to_vec(),
            gas_used: self.gas_used.as_u64(),
            gas_limit: self.gas_limit.as_u64(),
            bloom: self.bloom.as_bytes().to_vec(),
            timestamp: self.timestamp.as_u64(),
            parent_hash: self.parent_hash.as_fixed_bytes().to_vec(),
            author: self.author.to_fixed_bytes().to_vec(),
            extra_data: self.extra_data.clone().0,
            miner: self.miner.to_fixed_bytes().to_vec(),
            receipts_root: self.receipts_root.as_fixed_bytes().to_vec(),
            uncle_hash: self.uncle_hash.as_fixed_bytes().to_vec(),
            size: self.size.into(),
            state_root: self.state_root.as_fixed_bytes().to_vec(),
            transaction_hashes,
        }
    }

    pub fn from_append_entry_block(entry: append_entry::BlockEntry) -> anyhow::Result<Self> {
        type Error = anyhow::Error;
        Ok(BlockHeader {
            number: entry.number.into(),
            hash: Hash::new_from_h256(H256::from_slice(&entry.hash)),
            transactions_root: Hash::new_from_h256(H256::from_slice(&entry.transactions_root)),
            gas_used: Gas::from(entry.gas_used),
            gas_limit: Gas::from(entry.gas_limit),
            bloom: LogsBloom::from_bytes(&entry.bloom),
            timestamp: UnixTime::from(entry.timestamp),
            parent_hash: Hash::new_from_h256(H256::from_slice(&entry.parent_hash)),
            author: Address::new_from_h160(H160::from_slice(&entry.author)),
            extra_data: Bytes(entry.extra_data),
            miner: Address::new_from_h160(H160::from_slice(&entry.miner)),
            difficulty: Difficulty::from(U256::zero()), // always 0x0
            receipts_root: Hash::new_from_h256(H256::from_slice(&entry.receipts_root)),
            uncle_hash: Hash::new_from_h256(H256::from_slice(&entry.uncle_hash)),
            size: Size::from(entry.size),
            state_root: Hash::new_from_h256(H256::from_slice(&entry.state_root)),
            total_difficulty: Difficulty::from(U256::zero()), // always 0x0
            nonce: MinerNonce::default(),                     // always 0x0000000000000000
        })
    }
}

impl Dummy<Faker> for BlockHeader {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(faker: &Faker, rng: &mut R) -> Self {
        Self {
            number: faker.fake_with_rng(rng),
            hash: faker.fake_with_rng(rng),
            transactions_root: faker.fake_with_rng(rng),
            gas_used: faker.fake_with_rng(rng),
            bloom: Default::default(),
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
            parent_hash: header.parent_hash.into(),

            // mining: identifiers
            timestamp: (*header.timestamp).into(),
            author: Some(Address::COINBASE.into()),

            // minining: difficulty
            difficulty: U256::zero(),
            total_difficulty: Some(U256::zero()),
            nonce: Some(H64::zero()),

            // mining: gas
            gas_limit: Gas::from(100_000_000u64).into(),
            gas_used: header.gas_used.into(),
            base_fee_per_gas: Some(U256::zero()),
            blob_gas_used: None,
            excess_blob_gas: None,

            // transactions
            transactions_root: header.transactions_root.into(),
            receipts_root: HASH_EMPTY_TRIE.into(),

            // data
            logs_bloom: Some(*header.bloom),
            extra_data: Default::default(),

            // TODO
            ..Default::default() // state_root: todo!(),
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

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl TryFrom<&ExternalBlock> for BlockHeader {
    type Error = anyhow::Error;
    fn try_from(value: &ExternalBlock) -> Result<Self, Self::Error> {
        Ok(Self {
            number: value.number(),
            hash: value.hash(),
            transactions_root: value.transactions_root.into(),
            gas_used: value.gas_used.try_into()?,
            bloom: value.logs_bloom.unwrap_or_default().into(),
            timestamp: value.timestamp.into(),
            parent_hash: value.parent_hash.into(),
            gas_limit: value.gas_limit.try_into()?,
            author: value.author(),
            extra_data: value.extra_data.clone().into(),
            miner: value.author.unwrap_or_default().into(),
            difficulty: value.difficulty.into(),
            receipts_root: value.receipts_root.into(),
            uncle_hash: value.uncles_hash.into(),
            size: value.size.unwrap_or_default().try_into()?,
            state_root: value.state_root.into(),
            total_difficulty: value.total_difficulty.unwrap_or_default().into(),
            nonce: value.nonce.unwrap_or_default().into(),
        })
    }
}

impl From<BlockHeader> for SubscriptionMessage {
    fn from(value: BlockHeader) -> Self {
        let ethers_block: EthersBlock<()> = EthersBlock::from(value);
        Self::from_json(&ethers_block).unwrap()
    }
}
