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
use crate::eth::primitives::SlotValue;

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
