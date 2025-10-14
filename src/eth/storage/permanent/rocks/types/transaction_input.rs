use std::fmt::Debug;

use alloy_primitives::U64;
use alloy_primitives::U256;

use super::address::AddressRocksdb;
use super::bytes::BytesRocksdb;
use super::chain_id::ChainIdRocksdb;
use super::gas::GasRocksdb;
use super::hash::HashRocksdb;
use super::nonce::NonceRocksdb;
use super::wei::WeiRocksdb;
use crate::eth::primitives::ExecutionInfo;
use crate::eth::primitives::Signature;
use crate::eth::primitives::TransactionInfo;
use crate::eth::primitives::TransactionInput;
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;
use crate::ext::OptionExt;
use crate::ext::RuintExt;

#[derive(Debug, Clone, Default, PartialEq, Eq, bincode::Encode, bincode::Decode, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(fake::Dummy))]
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
            tx_type: item.transaction_info.tx_type.map(|inner| inner.as_u64() as u8),
            chain_id: item.execution_info.chain_id.map_into(),
            hash: HashRocksdb::from(item.transaction_info.hash),
            nonce: NonceRocksdb::from(item.execution_info.nonce),
            signer: AddressRocksdb::from(item.execution_info.signer),
            from: AddressRocksdb::from(item.execution_info.signer), // TODO: remove redundant field (requires reprocessing)
            to: item.execution_info.to.map(AddressRocksdb::from),
            value: WeiRocksdb::from(item.execution_info.value),
            input: BytesRocksdb::from(item.execution_info.input),
            gas_limit: GasRocksdb::from(item.execution_info.gas_limit),
            gas_price: WeiRocksdb::from(item.execution_info.gas_price),
            v: item.signature.v.as_u64(),
            r: item.signature.r.into_limbs(),
            s: item.signature.s.into_limbs(),
        }
    }
}

impl From<TransactionInputRocksdb> for TransactionInput {
    fn from(item: TransactionInputRocksdb) -> Self {
        Self {
            transaction_info: TransactionInfo {
                tx_type: item.tx_type.map(|typ| U64::from(typ)),
                hash: item.hash.into(),
            },
            execution_info: ExecutionInfo {
                chain_id: item.chain_id.map_into(),
                nonce: item.nonce.into(),
                signer: item.signer.into(),
                to: item.to.map(Into::into),
                value: item.value.into(),
                input: item.input.into(),
                gas_limit: item.gas_limit.into(),
                gas_price: item.gas_price.into(),
            },
            signature: Signature {
                v: U64::from(item.v),
                r: U256::from_limbs(item.r),
                s: U256::from_limbs(item.s),
            },
        }
    }
}

impl SerializeDeserializeWithContext for TransactionInputRocksdb {}
