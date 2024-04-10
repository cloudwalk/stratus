//! Slot Module
//!
//! Manages the concept of slots in Ethereum's state storage. A slot represents
//! a storage location in a smart contract, identified by an index and holding a
//! value. This module defines the structure of a slot and provides mechanisms
//! for interacting with slots, including reading and modifying storage data in
//! the context of Ethereum smart contracts.

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
use sqlx::Decode;

use super::Address;
use crate::eth::primitives::BlockNumber;
use crate::gen_newtype_from;

#[derive(Debug, Clone, Default, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct Slot {
    pub index: SlotIndex,
    pub value: SlotValue,
}

impl Slot {
    pub fn new(index: impl Into<SlotIndex>, value: impl Into<SlotValue>) -> Self {
        Self {
            index: index.into(),
            value: value.into(),
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

#[derive(Clone, Default, Eq, PartialEq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct SlotIndex(U256);

impl SlotIndex {
    pub const ZERO: SlotIndex = SlotIndex(U256::zero());

    /// Converts itself to [`U256`].
    pub fn as_u256(&self) -> U256 {
        self.0
    }
}

impl FromStr for SlotIndex {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self, Self::Err> {
        // This assumes that U256 has a from_str method that returns Result<U256, ParseIntError>
        let inner = U256::from_str(s)?;
        Ok(SlotIndex(inner))
    }
}

impl Dummy<Faker> for SlotIndex {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

impl Debug for SlotIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SlotIndex({:#x})", self.0)
    }
}

impl Display for SlotIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#x}", self.0)
    }
}

gen_newtype_from!(self = SlotIndex, other = u64, U256, [u8; 32]);

impl From<Vec<u8>> for SlotIndex {
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
        SlotIndex(u256)
    }
}

impl From<RevmU256> for SlotIndex {
    fn from(value: RevmU256) -> Self {
        Self(value.to_be_bytes().into())
    }
}

impl From<SlotIndex> for ethereum_types::U256 {
    fn from(value: SlotIndex) -> ethereum_types::U256 {
        value.0
    }
}

// -----------------------------------------------------------------------------
// sqlx traits for SlotIndex
// -----------------------------------------------------------------------------
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for SlotIndex {
    fn decode(value: <sqlx::Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <[u8; 32] as Decode<sqlx::Postgres>>::decode(value)?;
        Ok(value.into())
    }
}

impl sqlx::Type<sqlx::Postgres> for SlotIndex {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("BYTEA")
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for SlotIndex {
    fn encode_by_ref(&self, buf: &mut <sqlx::Postgres as HasArguments<'q>>::ArgumentBuffer) -> IsNull {
        <[u8; 32] as sqlx::Encode<sqlx::Postgres>>::encode(self.clone().into(), buf)
    }

    fn encode(self, buf: &mut <sqlx::Postgres as HasArguments<'q>>::ArgumentBuffer) -> IsNull
    where
        Self: Sized,
    {
        <[u8; 32] as sqlx::Encode<sqlx::Postgres>>::encode(self.into(), buf)
    }
}

impl PgHasArrayType for SlotIndex {
    fn array_type_info() -> sqlx::postgres::PgTypeInfo {
        <[u8; 32] as PgHasArrayType>::array_type_info()
    }
}

// -----------------------------------------------------------------------------
// Conversions: SlotIndex -> Other
// -----------------------------------------------------------------------------
impl From<SlotIndex> for [u8; 32] {
    fn from(value: SlotIndex) -> [u8; 32] {
        let mut buf: [u8; 32] = [1; 32];
        U256::from(value).to_big_endian(&mut buf);
        buf
    }
}

// -----------------------------------------------------------------------------
// SlotValue
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
        <[u8; 32] as sqlx::Encode<sqlx::Postgres>>::encode(self.clone().into(), buf)
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

#[derive(Debug, sqlx::Decode)]
pub struct SlotSample {
    pub address: Address,
    pub block_number: BlockNumber,
    pub index: SlotIndex,
    pub value: SlotValue,
}
