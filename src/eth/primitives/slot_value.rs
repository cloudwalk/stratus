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

use crate::gen_newtype_from;

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

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

gen_newtype_from!(self = SlotValue, other = u64, U256, [u8; 32]);

impl From<SlotValue> for RevmU256 {
    fn from(value: SlotValue) -> Self {
        RevmU256::from_limbs(value.0 .0)
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

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl From<RevmU256> for SlotValue {
    fn from(value: RevmU256) -> Self {
        Self(value.to_be_bytes().into())
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

impl FromStr for SlotValue {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let inner = U256::from_str(s)?;
        Ok(SlotValue(inner))
    }
}

// -----------------------------------------------------------------------------
// sqlx traits
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
