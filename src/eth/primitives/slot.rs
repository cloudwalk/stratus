use std::fmt::Display;

use ethereum_types::U256;
use fake::Dummy;
use fake::Faker;
use revm::primitives::U256 as RevmU256;
use sqlx::database::{HasArguments, HasValueRef};
use sqlx::encode::IsNull;
use sqlx::error::BoxDynError;
use sqlx::{Decode, Encode, Postgres};

use crate::derive_newtype_from;
use crate::eth::EthError;

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
}

impl Display for Slot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}={}", self.index, self.value)
    }
}

// -----------------------------------------------------------------------------
// SlotIndex
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SlotIndex(U256);

impl Dummy<Faker> for SlotIndex {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

impl Display for SlotIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#x}", self.0)
    }
}

derive_newtype_from!(self = SlotIndex, other = u64, U256);

impl From<RevmU256> for SlotIndex {
    fn from(value: RevmU256) -> Self {
        Self(value.to_be_bytes().into())
    }
}

impl TryFrom<Vec<u8>> for SlotIndex {
    type Error = EthError;

    fn try_from(value: Vec<u8>) -> Result<Self, EthError> {
        let value: [u8; 32] = value.try_into().unwrap();
        Ok(SlotIndex(U256::from(value)))
    }
}

impl From<SlotIndex> for ethereum_types::U256 {
    fn from(value: SlotIndex) -> ethereum_types::U256 {
        value.0
    }
}

// -----------------------------------------------------------------------------
// Conversions: sqlx -> SlotIndex
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

// -----------------------------------------------------------------------------
// Conversions: SlotIndex -> sqlx
// -----------------------------------------------------------------------------
impl<'q> sqlx::Encode<'q, sqlx::Postgres> for SlotIndex {
    fn encode_by_ref(&self, buf: &mut <sqlx::Postgres as HasArguments<'q>>::ArgumentBuffer) -> IsNull {
        self.0.to_little_endian(buf);
        self.encode(buf)
    }
}

// -----------------------------------------------------------------------------
// SlotValue
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SlotValue(U256);

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

derive_newtype_from!(self = SlotValue, other = u64, U256);

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

impl TryFrom<Vec<u8>> for SlotValue {
    type Error = EthError;

    fn try_from(value: Vec<u8>) -> Result<Self, EthError> {
        let value: [u8; 32] = value.try_into().unwrap();
        Ok(SlotValue(U256::from(value)))
    }
}

// -----------------------------------------------------------------------------
// Conversions: sqlx -> SlotValue
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
