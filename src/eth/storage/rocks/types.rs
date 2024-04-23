use std::fmt::Debug;
use std::fmt::Display;
use std::ops::Deref;

use ethereum_types::Bloom;
use ethereum_types::H160;
use ethereum_types::H256;
use ethereum_types::H64;
use ethereum_types::U256;
use ethereum_types::U64;
use revm::primitives::KECCAK_EMPTY;

use crate::eth::primitives::logs_bloom::LogsBloom;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockHeader;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::Difficulty;
use crate::eth::primitives::Execution;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Index;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::MinerNonce;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::Size;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotValue;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::TransactionMined;
use crate::eth::primitives::UnixTime;
use crate::eth::primitives::Wei;
use crate::ext::OptionExt;
use crate::gen_newtype_from;

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct AccountRocksdb {
    pub balance: WeiRocksdb,
    pub nonce: NonceRocksdb,
    pub bytecode: Option<BytesRocksdb>,
}

impl From<Account> for (AddressRocksdb, AccountRocksdb) {
    fn from(value: Account) -> Self {
        (
            value.address.into(),
            AccountRocksdb {
                balance: value.balance.into(),
                nonce: value.nonce.into(),
                bytecode: value.bytecode.map_into(),
            },
        )
    }
}

impl Default for AccountRocksdb {
    fn default() -> Self {
        Self {
            balance: WeiRocksdb::ZERO,
            nonce: NonceRocksdb::ZERO,
            bytecode: None,
        }
    }
}

#[derive(Clone, Default, Eq, PartialEq, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct BytesRocksdb(pub Vec<u8>);

impl Deref for BytesRocksdb {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for BytesRocksdb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.len() <= 256 {
            write!(f, "{}", const_hex::encode_prefixed(&self.0))
        } else {
            write!(f, "too long")
        }
    }
}

impl Debug for BytesRocksdb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Bytes").field(&self.to_string()).finish()
    }
}

impl From<Bytes> for BytesRocksdb {
    fn from(value: Bytes) -> Self {
        Self(value.0)
    }
}

impl From<BytesRocksdb> for Bytes {
    fn from(value: BytesRocksdb) -> Self {
        Self(value.0)
    }
}

#[derive(Debug, Clone, Default, Eq, PartialEq, derive_more::Add, derive_more::Sub, serde::Serialize, serde::Deserialize)]
pub struct WeiRocksdb(U256);

gen_newtype_from!(self = WeiRocksdb, other = U256);

impl From<WeiRocksdb> for Wei {
    fn from(value: WeiRocksdb) -> Self {
        value.0.into()
    }
}

impl From<Wei> for WeiRocksdb {
    fn from(value: Wei) -> Self {
        U256::from(value).into()
    }
}

impl WeiRocksdb {
    pub const ZERO: WeiRocksdb = WeiRocksdb(U256::zero());
    pub const ONE: WeiRocksdb = WeiRocksdb(U256::one());
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NonceRocksdb(U64);

gen_newtype_from!(self = NonceRocksdb, other = u64);

impl From<NonceRocksdb> for Nonce {
    fn from(value: NonceRocksdb) -> Self {
        value.0.as_u64().into()
    }
}

impl From<Nonce> for NonceRocksdb {
    fn from(value: Nonce) -> Self {
        u64::from(value).into()
    }
}

impl NonceRocksdb {
    pub const ZERO: NonceRocksdb = NonceRocksdb(U64::zero());
}

impl AccountRocksdb {
    pub fn to_account(&self, address: &Address) -> Account {
        Account {
            address: *address,
            nonce: self.nonce.clone().into(),
            balance: self.balance.clone().into(),
            bytecode: self.bytecode.clone().map_into(),
            code_hash: KECCAK_EMPTY.into(),
            static_slot_indexes: None,  // TODO: is it necessary for RocksDB?
            mapping_slot_indexes: None, // TODO: is it necessary for RocksDB?
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SlotValueRocksdb(U256);

impl SlotValueRocksdb {
    pub fn inner_value(&self) -> U256 {
        self.0
    }
}

impl From<SlotValue> for SlotValueRocksdb {
    fn from(item: SlotValue) -> Self {
        SlotValueRocksdb(item.inner_value())
    }
}

impl From<SlotValueRocksdb> for SlotValue {
    fn from(item: SlotValueRocksdb) -> Self {
        SlotValue::new(item.inner_value())
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AddressRocksdb(H160);

impl AddressRocksdb {
    pub fn inner_value(&self) -> H160 {
        self.0
    }
}

impl From<Address> for AddressRocksdb {
    fn from(item: Address) -> Self {
        AddressRocksdb(item.inner_value())
    }
}

impl From<AddressRocksdb> for Address {
    fn from(item: AddressRocksdb) -> Self {
        Address::new_from_h160(item.inner_value())
    }
}

#[derive(Debug, Clone, Default, Eq, PartialEq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub struct BlockNumberRocksdb(U64);

gen_newtype_from!(self = BlockNumberRocksdb, other = u8, u16, u32, u64, U64, usize, i32, i64);
impl BlockNumberRocksdb {
    pub fn inner_value(&self) -> U64 {
        self.0
    }
}

impl From<BlockNumber> for BlockNumberRocksdb {
    fn from(item: BlockNumber) -> Self {
        BlockNumberRocksdb(item.inner_value())
    }
}

impl From<BlockNumberRocksdb> for BlockNumber {
    fn from(item: BlockNumberRocksdb) -> Self {
        BlockNumber::from(item.inner_value())
    }
}

#[derive(Clone, Default, Hash, Eq, PartialEq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct SlotIndexRocksdb(U256);

impl SlotIndexRocksdb {
    pub fn inner_value(&self) -> U256 {
        self.0
    }
}

impl From<SlotIndex> for SlotIndexRocksdb {
    fn from(item: SlotIndex) -> Self {
        SlotIndexRocksdb(item.inner_value())
    }
}

impl From<SlotIndexRocksdb> for SlotIndex {
    fn from(item: SlotIndexRocksdb) -> Self {
        SlotIndex::new(item.inner_value())
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct HashRocksdb(H256);

impl HashRocksdb {
    pub fn inner_value(&self) -> H256 {
        self.0
    }
}

impl From<Hash> for HashRocksdb {
    fn from(item: Hash) -> Self {
        HashRocksdb(item.inner_value())
    }
}

impl From<HashRocksdb> for Hash {
    fn from(item: HashRocksdb) -> Self {
        Hash::new_from_h256(item.inner_value())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize, derive_more::Add, Copy, Hash)]
pub struct IndexRocksdb(u64);

impl IndexRocksdb {
    pub fn inner_value(&self) -> u64 {
        self.0
    }
}

impl From<Index> for IndexRocksdb {
    fn from(item: Index) -> Self {
        IndexRocksdb(item.inner_value())
    }
}

impl From<IndexRocksdb> for Index {
    fn from(item: IndexRocksdb) -> Self {
        Index::new(item.inner_value())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct GasRocksdb(U64);

gen_newtype_from!(self = GasRocksdb, other = u64);

impl From<GasRocksdb> for Gas {
    fn from(value: GasRocksdb) -> Self {
        value.0.as_u64().into()
    }
}

impl From<Gas> for GasRocksdb {
    fn from(value: Gas) -> Self {
        u64::from(value).into()
    }
}

#[derive(Debug, Clone, Default, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct MinerNonceRocksdb(H64);

gen_newtype_from!(self = MinerNonceRocksdb, other = H64, [u8; 8], MinerNonce);

impl From<MinerNonceRocksdb> for MinerNonce {
    fn from(value: MinerNonceRocksdb) -> Self {
        value.0.into()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct DifficultyRocksdb(U256);

gen_newtype_from!(self = DifficultyRocksdb, other = U256);

impl From<DifficultyRocksdb> for Difficulty {
    fn from(value: DifficultyRocksdb) -> Self {
        value.0.into()
    }
}

impl From<Difficulty> for DifficultyRocksdb {
    fn from(value: Difficulty) -> Self {
        U256::from(value).into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BlockHeaderRocksdb {
    pub number: BlockNumberRocksdb,
    pub hash: HashRocksdb,
    pub transactions_root: HashRocksdb,
    pub gas_used: GasRocksdb,
    pub gas_limit: GasRocksdb,
    pub bloom: LogsBloomRocksdb,
    pub timestamp: UnixTimeRocksdb,
    pub parent_hash: HashRocksdb,
    pub author: AddressRocksdb,
    pub extra_data: BytesRocksdb,
    pub miner: AddressRocksdb,
    pub difficulty: DifficultyRocksdb,
    pub receipts_root: HashRocksdb,
    pub uncle_hash: HashRocksdb,
    pub size: SizeRocksdb,
    pub state_root: HashRocksdb,
    pub total_difficulty: DifficultyRocksdb,
    pub nonce: MinerNonceRocksdb,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UnixTimeRocksdb(u64);

gen_newtype_from!(self = UnixTimeRocksdb, other = u64);

impl From<UnixTime> for UnixTimeRocksdb {
    fn from(value: UnixTime) -> Self {
        Self(*value)
    }
}

impl From<UnixTimeRocksdb> for UnixTime {
    fn from(value: UnixTimeRocksdb) -> Self {
        value.0.into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(transparent)]
pub struct LogsBloomRocksdb(Bloom);

gen_newtype_from!(self = LogsBloomRocksdb, other = Bloom, LogsBloom);

impl From<LogsBloomRocksdb> for LogsBloom {
    fn from(value: LogsBloomRocksdb) -> Self {
        value.0.into()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct SizeRocksdb(U64);

gen_newtype_from!(self = SizeRocksdb, other = U64, u64);

impl From<Size> for SizeRocksdb {
    fn from(value: Size) -> Self {
        u64::from(value).into()
    }
}

impl From<SizeRocksdb> for Size {
    fn from(value: SizeRocksdb) -> Self {
        value.0.as_u64().into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TransactionMinedRocksdb {
    pub input: TransactionInput,
    pub execution: Execution,
    pub logs: Vec<LogMined>,
    pub transaction_index: IndexRocksdb,
    pub block_number: BlockNumberRocksdb,
    pub block_hash: HashRocksdb,
}

impl From<TransactionMined> for TransactionMinedRocksdb {
    fn from(item: TransactionMined) -> Self {
        Self {
            input: item.input,
            execution: item.execution,
            logs: item.logs,
            transaction_index: IndexRocksdb::from(item.transaction_index),
            block_number: BlockNumberRocksdb::from(item.block_number),
            block_hash: HashRocksdb::from(item.block_hash),
        }
    }
}

impl From<TransactionMinedRocksdb> for TransactionMined {
    fn from(item: TransactionMinedRocksdb) -> Self {
        Self {
            input: item.input,
            execution: item.execution,
            logs: item.logs,
            transaction_index: item.transaction_index.into(),
            block_number: item.block_number.into(),
            block_hash: item.block_hash.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BlockRocksdb {
    pub header: BlockHeaderRocksdb,
    pub transactions: Vec<TransactionMinedRocksdb>,
}

impl From<Block> for BlockRocksdb {
    fn from(item: Block) -> Self {
        BlockRocksdb {
            header: BlockHeaderRocksdb {
                number: BlockNumberRocksdb::from(item.header.number),
                hash: HashRocksdb::from(item.header.hash),
                transactions_root: HashRocksdb::from(item.header.transactions_root),
                gas_used: item.header.gas_used.into(),
                gas_limit: item.header.gas_limit.into(),
                bloom: item.header.bloom.into(),
                timestamp: item.header.timestamp.into(),
                parent_hash: HashRocksdb::from(item.header.parent_hash),
                author: AddressRocksdb::from(item.header.author),
                extra_data: item.header.extra_data.into(),
                miner: AddressRocksdb::from(item.header.miner),
                difficulty: item.header.difficulty.into(),
                receipts_root: HashRocksdb::from(item.header.receipts_root),
                uncle_hash: HashRocksdb::from(item.header.uncle_hash),
                size: item.header.size.into(),
                state_root: HashRocksdb::from(item.header.state_root),
                total_difficulty: item.header.total_difficulty.into(),
                nonce: item.header.nonce.into(),
            },
            transactions: item.transactions.into_iter().map(TransactionMinedRocksdb::from).collect(),
        }
    }
}

impl From<BlockRocksdb> for Block {
    fn from(item: BlockRocksdb) -> Self {
        Block {
            header: BlockHeader {
                number: BlockNumber::from(item.header.number),
                hash: Hash::from(item.header.hash),
                transactions_root: Hash::from(item.header.transactions_root),
                gas_used: item.header.gas_used.into(),
                gas_limit: item.header.gas_limit.into(),
                bloom: item.header.bloom.into(),
                timestamp: item.header.timestamp.into(),
                parent_hash: Hash::from(item.header.parent_hash),
                author: Address::from(item.header.author),
                extra_data: item.header.extra_data.into(),
                miner: Address::from(item.header.miner),
                difficulty: item.header.difficulty.into(),
                receipts_root: Hash::from(item.header.receipts_root),
                uncle_hash: Hash::from(item.header.uncle_hash),
                size: item.header.size.into(),
                state_root: Hash::from(item.header.state_root),
                total_difficulty: item.header.total_difficulty.into(),
                nonce: item.header.nonce.into(),
            },
            transactions: item.transactions.into_iter().map(TransactionMined::from).collect(),
        }
    }
}
