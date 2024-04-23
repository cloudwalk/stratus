use std::fmt::Display;

use ethereum_types::H64;
use fake::Dummy;
use fake::Faker;
use sqlx::database::HasArguments;
use sqlx::database::HasValueRef;
use sqlx::encode::IsNull;
use sqlx::error::BoxDynError;
use sqlx::postgres::PgHasArrayType;
use sqlx::Decode;

use crate::gen_newtype_from;

/// The nonce of an Ethereum block.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct MinerNonce(H64);

impl MinerNonce {
    /// Creates a new BlockNonce from the given bytes.
    pub const fn new(bytes: [u8; 8]) -> Self {
        Self(H64(bytes))
    }
}

impl Dummy<Faker> for MinerNonce {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        H64::random_using(rng).into()
    }
}

impl Display for MinerNonce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = MinerNonce, other = H64, [u8; 8]);

// -----------------------------------------------------------------------------
// sqlx traits
// -----------------------------------------------------------------------------
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for MinerNonce {
    fn decode(value: <sqlx::Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <[u8; 8] as Decode<sqlx::Postgres>>::decode(value)?;
        Ok(value.into())
    }
}

impl sqlx::Type<sqlx::Postgres> for MinerNonce {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("BYTEA")
    }
}

impl PgHasArrayType for MinerNonce {
    fn array_type_info() -> sqlx::postgres::PgTypeInfo {
        <[u8; 8] as PgHasArrayType>::array_type_info()
    }
}

impl AsRef<[u8]> for MinerNonce {
    fn as_ref(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for MinerNonce {
    fn encode_by_ref(&self, buf: &mut <sqlx::Postgres as HasArguments<'q>>::ArgumentBuffer) -> IsNull {
        self.0 .0.encode(buf)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<MinerNonce> for H64 {
    fn from(value: MinerNonce) -> Self {
        value.0
    }
}

impl From<MinerNonce> for [u8; 8] {
    fn from(value: MinerNonce) -> Self {
        H64::from(value).0
    }
}
