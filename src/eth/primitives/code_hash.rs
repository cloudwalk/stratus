use ethereum_types::H256;
use ethers_core::utils::keccak256;
use fake::Dummy;
use fake::Faker;
use revm::primitives::FixedBytes;
use revm::primitives::KECCAK_EMPTY;
use sqlx::database::HasValueRef;
use sqlx::error::BoxDynError;
use sqlx::postgres::PgHasArrayType;

use crate::eth::primitives::Bytes;
use crate::gen_newtype_from;

/// Digest of the bytecode of a contract.
/// In the case of an externally-owned account (EOA), bytecode is null
/// and the code hash is fixed as the keccak256 hash of an empty string
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CodeHash(H256);

impl Dummy<Faker> for CodeHash {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        Self(H256::random_using(rng))
    }
}

impl CodeHash {
    pub fn new(inner: H256) -> Self {
        CodeHash(inner)
    }

    pub fn from_bytecode(maybe_bytecode: Option<Bytes>) -> Self {
        match maybe_bytecode {
            Some(bytecode) => CodeHash(H256::from_slice(&keccak256(bytecode.as_ref()))),
            None => CodeHash::default(),
        }
    }

    pub fn inner(&self) -> H256 {
        self.0
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = CodeHash, other = [u8; 32]);

impl Default for CodeHash {
    fn default() -> Self {
        CodeHash(KECCAK_EMPTY.0.into())
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> other
// -----------------------------------------------------------------------------
impl AsRef<[u8]> for CodeHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl From<FixedBytes<32>> for CodeHash {
    fn from(value: FixedBytes<32>) -> Self {
        CodeHash::new(value.0.into())
    }
}

// -----------------------------------------------------------------------------
// sqlx traits
// -----------------------------------------------------------------------------
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for CodeHash {
    fn decode(value: <sqlx::Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <[u8; 32] as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        Ok(value.into())
    }
}

impl sqlx::Type<sqlx::Postgres> for CodeHash {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("BYTEA")
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for CodeHash {
    fn encode(self, buf: &mut <sqlx::Postgres as sqlx::database::HasArguments<'q>>::ArgumentBuffer) -> sqlx::encode::IsNull
    where
        Self: Sized,
    {
        <&[u8; 32] as sqlx::Encode<sqlx::Postgres>>::encode(self.0.as_fixed_bytes(), buf)
    }

    fn encode_by_ref(&self, buf: &mut <sqlx::Postgres as sqlx::database::HasArguments<'q>>::ArgumentBuffer) -> sqlx::encode::IsNull {
        <&[u8; 32] as sqlx::Encode<sqlx::Postgres>>::encode(self.0.as_fixed_bytes(), buf)
    }
}

impl PgHasArrayType for CodeHash {
    fn array_type_info() -> sqlx::postgres::PgTypeInfo {
        <&[u8; 32] as PgHasArrayType>::array_type_info()
    }
}
