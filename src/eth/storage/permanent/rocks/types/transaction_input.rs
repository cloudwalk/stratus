use std::fmt::Debug;

use ethereum_types::U256;

use super::address::AddressRocksdb;
use super::bytes::BytesRocksdb;
use super::chain_id::ChainIdRocksdb;
use super::gas::GasRocksdb;
use super::hash::HashRocksdb;
use super::nonce::NonceRocksdb;
use super::wei::WeiRocksdb;
use crate::eth::primitives::TransactionInput;
use crate::ext::OptionExt;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub struct TransactionInputRocksdb {
    pub tx_type: Option<u8>,
    pub chain_id: Option<ChainIdRocksdb>,
    pub hash: HashRocksdb,
    pub nonce: NonceRocksdb,
    pub signer: AddressRocksdb,
    pub from: AddressRocksdb,
    pub to: Option<AddressRocksdb>,
    pub value: WeiRocksdb,
    pub input: BytesRocksdb,
    pub gas_limit: GasRocksdb,
    pub gas_price: WeiRocksdb,
    pub v: u64,
    pub r: [u64; 4],
    pub s: [u64; 4],
}

impl From<TransactionInput> for TransactionInputRocksdb {
    fn from(item: TransactionInput) -> Self {
        Self {
            tx_type: item.tx_type.map(|inner| inner.as_u64() as u8),
            chain_id: item.chain_id.map_into(),
            hash: HashRocksdb::from(item.hash),
            nonce: NonceRocksdb::from(item.nonce),
            signer: AddressRocksdb::from(item.signer),
            from: AddressRocksdb::from(item.from),
            to: item.to.map(AddressRocksdb::from),
            value: WeiRocksdb::from(item.value),
            input: BytesRocksdb::from(item.input),
            gas_limit: GasRocksdb::from(item.gas_limit),
            gas_price: WeiRocksdb::from(item.gas_price),
            v: item.v.as_u64(),
            r: item.r.0,
            s: item.s.0,
        }
    }
}

impl From<TransactionInputRocksdb> for TransactionInput {
    fn from(item: TransactionInputRocksdb) -> Self {
        Self {
            chain_id: item.chain_id.map_into(),
            hash: item.hash.into(),
            nonce: item.nonce.into(),
            signer: item.signer.into(),
            from: item.from.into(),
            to: item.to.map(Into::into),
            value: item.value.into(),
            input: item.input.into(),
            gas_limit: item.gas_limit.into(),
            gas_price: item.gas_price.into(),
            v: item.v.into(),
            r: U256(item.r),
            s: U256(item.s),
            tx_type: item.tx_type.map_into(),
        }
    }
}
