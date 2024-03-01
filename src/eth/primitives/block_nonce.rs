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

/// BlockNonce of an Ethereum account (wallet or contract).
#[derive(Debug, Clone, Default, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BlockNonce(H64);

impl BlockNonce {
    /// Creates a new BlockNonce from the given bytes.
    pub const fn new(bytes: [u8; 8]) -> Self {
        Self(H64(bytes))
    }
}

impl Dummy<Faker> for BlockNonce {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        H64::random_using(rng).into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = BlockNonce, other = H64, [u8; 8]);

// -----------------------------------------------------------------------------
// sqlx traits
// -----------------------------------------------------------------------------
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for BlockNonce {
    fn decode(value: <sqlx::Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <[u8; 8] as Decode<sqlx::Postgres>>::decode(value)?;
        Ok(value.into())
    }
}

impl sqlx::Type<sqlx::Postgres> for BlockNonce {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("BYTEA")
    }
}

impl PgHasArrayType for BlockNonce {
    fn array_type_info() -> sqlx::postgres::PgTypeInfo {
        <[u8; 8] as PgHasArrayType>::array_type_info()
    }
}

impl AsRef<[u8]> for BlockNonce {
    fn as_ref(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for BlockNonce {
    fn encode_by_ref(&self, buf: &mut <sqlx::Postgres as HasArguments<'q>>::ArgumentBuffer) -> IsNull {
        self.0 .0.encode(buf)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<BlockNonce> for H64 {
    fn from(value: BlockNonce) -> Self {
        value.0
    }
}

impl From<BlockNonce> for [u8; 8] {
    fn from(value: BlockNonce) -> Self {
        H64::from(value.clone()).0
    }
}
