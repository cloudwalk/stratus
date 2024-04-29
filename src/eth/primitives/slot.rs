//! Slot Module
//!
//! Manages the concept of slots in Ethereum's state storage. A slot represents
//! a storage location in a smart contract, identified by an index and holding a
//! value. This module defines the structure of a slot and provides mechanisms
//! for interacting with slots, including reading and modifying storage data in
//! the context of Ethereum smart contracts.

use std::collections::HashSet;
use std::fmt::Debug;
use std::fmt::Display;
use std::str::FromStr;

use ethereum_types::U256;
use fake::Dummy;
use fake::Faker;
use revm::primitives::U256 as RevmU256;
use sqlx::database::HasArguments;
use sqlx::database::HasValueRef;
use sqlx::encode::IsNull;
use sqlx::error::BoxDynError;
use sqlx::postgres::PgHasArrayType;
use sqlx::types::Json;
use sqlx::Decode;

use super::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::SlotIndex;
use crate::gen_newtype_from;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct Slot {
    pub index: SlotIndex,
    pub value: SlotValue,
}

impl Slot {
    /// Creates a new slot with the given index and value.
    pub fn new(index: SlotIndex, value: SlotValue) -> Self {
        Self { index, value }
    }

    /// Creates a new slot with the given index and default zero value.
    pub fn new_empty(index: SlotIndex) -> Self {
        Self {
            index,
            value: SlotValue::default(),
        }
    }

    /// Checks if the value is zero.
    pub fn is_zero(&self) -> bool {
        self.value.is_zero()
    }
}

impl Display for Slot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}={}", self.index, self.value)
    }
}

// -----------------------------------------------------------------------------
// SlotIndex
// -----------------------------------------------------------------------------

// -----------------------------------------------------------------------------
// SlotValue
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SlotValue(U256);

impl SlotValue {
    /// Checks if the value is zero.
    pub fn is_zero(&self) -> bool {
        self.0.is_zero()
    }

    /// Converts itself to [`U256`].
    pub fn as_u256(&self) -> U256 {
        self.0
    }

    pub fn new(value: U256) -> Self {
        SlotValue(value)
    }

    pub fn inner_value(&self) -> U256 {
        self.0
    }
}

impl FromStr for SlotValue {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let inner = U256::from_str(s)?;
        Ok(SlotValue(inner))
    }
}

impl Display for SlotValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#x}", self.0)
    }
}

impl Dummy<Faker> for SlotValue {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

impl From<SlotValue> for [u8; 32] {
    fn from(value: SlotValue) -> Self {
        let mut buf: [u8; 32] = [1; 32];
        value.0.to_big_endian(&mut buf);
        buf
    }
}

impl From<SlotValue> for Vec<u8> {
    fn from(value: SlotValue) -> Self {
        let mut vec = vec![0u8; 32]; // Initialize a vector with 32 bytes set to 0
        value.0.to_big_endian(&mut vec);
        vec
    }
}
impl From<Vec<u8>> for SlotValue {
    fn from(bytes: Vec<u8>) -> Self {
        // Initialize U256 to zero
        // Assuming the byte array is in big-endian format,
        let u256: U256 = if bytes.len() <= 32 {
            let mut padded_bytes = [0u8; 32];
            padded_bytes[32 - bytes.len()..].copy_from_slice(&bytes);
            U256::from_big_endian(&padded_bytes)
        } else {
            // Handle the error or truncate the Vec<u8> as needed
            // For simplicity, this example will only load the first 32 bytes if the Vec is too large
            U256::from_big_endian(&bytes[0..32])
        };
        SlotValue(u256)
    }
}

gen_newtype_from!(self = SlotValue, other = u64, U256, [u8; 32]);

impl From<RevmU256> for SlotValue {
    fn from(value: RevmU256) -> Self {
        Self(value.to_be_bytes().into())
    }
}

impl From<SlotValue> for RevmU256 {
    fn from(value: SlotValue) -> Self {
        RevmU256::from_limbs(value.0 .0)
    }
}

// -----------------------------------------------------------------------------
// sqlx traits for SlotValue
// -----------------------------------------------------------------------------
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for SlotValue {
    fn decode(value: <sqlx::Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <[u8; 32] as Decode<sqlx::Postgres>>::decode(value)?;
        Ok(value.into())
    }
}

impl sqlx::Type<sqlx::Postgres> for SlotValue {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("BYTEA")
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for SlotValue {
    fn encode_by_ref(&self, buf: &mut <sqlx::Postgres as HasArguments<'q>>::ArgumentBuffer) -> IsNull {
        <[u8; 32] as sqlx::Encode<sqlx::Postgres>>::encode((*self).into(), buf)
    }

    fn encode(self, buf: &mut <sqlx::Postgres as HasArguments<'q>>::ArgumentBuffer) -> IsNull
    where
        Self: Sized,
    {
        <[u8; 32] as sqlx::Encode<sqlx::Postgres>>::encode(self.into(), buf)
    }
}

impl PgHasArrayType for SlotValue {
    fn array_type_info() -> sqlx::postgres::PgTypeInfo {
        <[u8; 32] as PgHasArrayType>::array_type_info()
    }
}

// -----------------------------------------------------------------------------
// SlotSample
// -----------------------------------------------------------------------------

#[derive(Debug, sqlx::Decode)]
pub struct SlotSample {
    pub address: Address,
    pub block_number: BlockNumber,
    pub index: SlotIndex,
    pub value: SlotValue,
}

// -----------------------------------------------------------------------------
// SlotAccess
// -----------------------------------------------------------------------------

/// How a slot is accessed.
#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum SlotAccess {
    /// Slot index will be accessed statically without any hashing.
    Static(SlotIndex),

    /// Index will be hashed according to mapping hash algorithm.
    Mapping(SlotIndex),

    /// Index will be hashed according to array hashing algorithm.
    Array(SlotIndex),
}

impl Display for SlotAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SlotAccess::Static(index) => write!(f, "Static  = {}", index),
            SlotAccess::Mapping(index) => write!(f, "Mapping = {}", index),
            SlotAccess::Array(index) => write!(f, "Array   = {}", index),
        }
    }
}

#[cfg(test)]
mod tests {
    use hex_literal::hex;

    use crate::eth::primitives::SlotIndex;

    #[test]
    fn slot_index_to_mapping_index() {
        let address = hex!("3c44cdddb6a900fa2b585dd299e03d12fa4293bc").to_vec();
        let hashed = SlotIndex::ZERO.to_mapping_index(address);
        assert_eq!(hashed.to_string(), "0x215be5d23550ceb1beff54fb579a765903ba2ccc85b6f79bcf9bda4e8cb86034");
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, fake::Dummy, serde::Deserialize, serde::Serialize, derive_more::Deref, derive_more::DerefMut)]
pub struct SlotIndexes(#[deref] pub HashSet<SlotIndex>);

impl SlotIndexes {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self(HashSet::with_capacity(capacity))
    }
}

// -----------------------------------------------------------------------------
// sqlx traits for SlotIndexes
// -----------------------------------------------------------------------------
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for SlotIndexes {
    fn decode(value: <sqlx::Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <Json<SlotIndexes> as Decode<sqlx::Postgres>>::decode(value)?.0;
        Ok(value)
    }
}

impl sqlx::Type<sqlx::Postgres> for SlotIndexes {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("JSONB")
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for SlotIndexes {
    fn encode_by_ref(&self, buf: &mut <sqlx::Postgres as HasArguments<'q>>::ArgumentBuffer) -> IsNull {
        <Json<SlotIndexes> as sqlx::Encode<sqlx::Postgres>>::encode(self.clone().into(), buf)
    }

    fn encode(self, buf: &mut <sqlx::Postgres as HasArguments<'q>>::ArgumentBuffer) -> IsNull
    where
        Self: Sized,
    {
        <Json<SlotIndexes> as sqlx::Encode<sqlx::Postgres>>::encode(self.into(), buf)
    }
}

impl PgHasArrayType for SlotIndexes {
    fn array_type_info() -> sqlx::postgres::PgTypeInfo {
        <Json<SlotIndexes> as PgHasArrayType>::array_type_info()
    }
}
