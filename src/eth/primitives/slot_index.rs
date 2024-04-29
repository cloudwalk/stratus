use std::fmt::Debug;
use std::fmt::Display;
use std::io::Read;
use std::str::FromStr;

use ethereum_types::U256;
use ethers_core::utils::keccak256;
use fake::Dummy;
use fake::Faker;
use revm::primitives::U256 as RevmU256;
use sqlx::database::HasArguments;
use sqlx::database::HasValueRef;
use sqlx::encode::IsNull;
use sqlx::error::BoxDynError;
use sqlx::postgres::PgHasArrayType;
use sqlx::Decode;

use crate::gen_newtype_from;

#[derive(Clone, Copy, Default, Hash, Eq, PartialEq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct SlotIndex(U256);

impl SlotIndex {
    pub const ZERO: SlotIndex = SlotIndex(U256::zero());
    pub const ONE: SlotIndex = SlotIndex(U256::one());

    /// Converts itself to [`U256`].
    pub fn as_u256(&self) -> U256 {
        self.0
    }

    /// Computes the mapping index of a key.
    pub fn to_mapping_index(&self, key: Vec<u8>) -> SlotIndex {
        // populate self to bytes
        let mut slot_index_bytes = [0u8; 32];
        self.0.to_big_endian(&mut slot_index_bytes);

        // populate key to bytes
        let mut key_bytes = [0u8; 32];
        let _ = key.take(32).read(&mut key_bytes[32usize.saturating_sub(key.len())..32]);

        // populate value to be hashed to bytes
        let mut mapping_index_bytes = [0u8; 64];
        mapping_index_bytes[0..32].copy_from_slice(&key_bytes);
        mapping_index_bytes[32..64].copy_from_slice(&slot_index_bytes);

        let hashed_bytes = keccak256(mapping_index_bytes);
        Self::from(hashed_bytes)
    }
}

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

impl Debug for SlotIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SlotIndex({:#x})", self.0)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

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

impl FromStr for SlotIndex {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self, Self::Err> {
        // This assumes that U256 has a from_str method that returns Result<U256, ParseIntError>
        let inner = U256::from_str(s)?;
        Ok(SlotIndex(inner))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<SlotIndex> for [u8; 32] {
    fn from(value: SlotIndex) -> [u8; 32] {
        let mut buf: [u8; 32] = [1; 32];
        U256::from(value).to_big_endian(&mut buf);
        buf
    }
}

impl From<SlotIndex> for Vec<u8> {
    fn from(value: SlotIndex) -> Self {
        let mut vec = vec![0u8; 32];
        value.0.to_big_endian(&mut vec);
        vec
    }
}

impl From<SlotIndex> for ethereum_types::U256 {
    fn from(value: SlotIndex) -> ethereum_types::U256 {
        value.0
    }
}

// -----------------------------------------------------------------------------
// sqlx traits
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
        <[u8; 32] as sqlx::Encode<sqlx::Postgres>>::encode((*self).into(), buf)
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
