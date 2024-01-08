use std::fmt::Display;
use std::num::TryFromIntError;
use std::str::FromStr;

use ethereum_types::U64;
use fake::Dummy;
use fake::Faker;
use sqlx::database::HasValueRef;
use sqlx::error::BoxDynError;

use crate::derive_newtype_from;
use crate::eth::EthError;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Add, derive_more::Sub, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct BlockNumber(U64);

impl BlockNumber {
    pub const ZERO: BlockNumber = BlockNumber(U64::zero());
}

impl Display for BlockNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Dummy<Faker> for BlockNumber {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
derive_newtype_from!(self = BlockNumber, other = u8, u16, u32, u64, U64, usize, i32, i64);

impl FromStr for BlockNumber {
    type Err = EthError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match U64::from_str(s) {
            Ok(parsed) => Ok(Self(parsed)),
            Err(e) => {
                tracing::warn!(reason = ?e, value = %s, "failed to parse block number");
                Err(EthError::new_invalid_field("blockNumber", s.to_owned()))
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Conversions: sqlx -> Self
// -----------------------------------------------------------------------------
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for BlockNumber {
    fn decode(value: <sqlx::Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <i64 as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        Ok(value.into())
    }
}

impl sqlx::Type<sqlx::Postgres> for BlockNumber {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("BYTEA")
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<BlockNumber> for U64 {
    fn from(block_number: BlockNumber) -> Self {
        block_number.0
    }
}

impl TryFrom<BlockNumber> for i64 {
    type Error = TryFromIntError;

    fn try_from(block_number: BlockNumber) -> Result<i64, TryFromIntError> {
        block_number.0 .0[0].try_into()
    }
}
