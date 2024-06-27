use std::fmt::Display;
use std::str::FromStr;

use anyhow::anyhow;
use ethereum_types::H256;
use fake::Dummy;
use fake::Faker;
use sqlx::database::HasValueRef;
use sqlx::error::BoxDynError;
use sqlx::postgres::PgHasArrayType;

use crate::gen_newtype_from;

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Hash(H256);

impl Hash {
    /// Creates a hash from the given bytes.
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(H256(bytes))
    }

    pub fn new_from_h256(h256: H256) -> Self {
        Self(h256)
    }

    /// Creates a new random hash.
    pub fn new_random() -> Self {
        Self(H256::random())
    }

    /// Returns the zero hash
    pub fn zero() -> Self {
        Self(H256::zero())
    }

    pub fn into_hash_partition(self) -> i16 {
        let n = self.0.to_low_u64_ne() % 10;
        n as i16
    }

    pub fn inner_value(&self) -> H256 {
        self.0
    }

    pub fn as_fixed_bytes(&self) -> &[u8; 32] {
        self.0.as_fixed_bytes()
    }
}

impl Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", const_hex::encode_prefixed(self.0))
    }
}

impl Dummy<Faker> for Hash {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        H256::random_using(rng).into()
    }
}

impl AsRef<[u8]> for Hash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = Hash, other = H256, [u8; 32]);

impl FromStr for Hash {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        match H256::from_str(s) {
            Ok(parsed) => Ok(Self(parsed)),
            Err(e) => {
                tracing::warn!(reason = ?e, value = %s, "failed to parse hash");
                Err(anyhow!("Failed to parse field 'hash' with value '{}'", s.to_owned()))
            }
        }
    }
}

// -----------------------------------------------------------------------------
// sqlx traits
// -----------------------------------------------------------------------------
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for Hash {
    fn decode(value: <sqlx::Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <[u8; 32] as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        Ok(value.into())
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for Hash {
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

impl sqlx::Type<sqlx::Postgres> for Hash {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("BYTEA")
    }
}

impl PgHasArrayType for Hash {
    fn array_type_info() -> sqlx::postgres::PgTypeInfo {
        <&[u8; 32] as PgHasArrayType>::array_type_info()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

impl From<Hash> for H256 {
    fn from(value: Hash) -> Self {
        value.0
    }
}
