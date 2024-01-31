//! Block Number Module
//!
//! The Block Number module manages the numerical identification of blocks in
//! the Ethereum blockchain. Each block is assigned a unique, sequential number
//! for easy reference and tracking within the blockchain. This module provides
//! functionality for manipulating and comparing block numbers, essential for
//! operations like validating blockchain continuity and retrieving specific
//! blocks.

use std::fmt::Display;
use std::num::TryFromIntError;
use std::str::FromStr;

use anyhow::anyhow;
use ethereum_types::U64;
use fake::Dummy;
use fake::Faker;
use sqlx::database::HasValueRef;
use sqlx::error::BoxDynError;

use crate::gen_newtype_from;

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
gen_newtype_from!(self = BlockNumber, other = u8, u16, u32, u64, U64, usize, i32, i64);

impl FromStr for BlockNumber {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        match U64::from_str(s) {
            Ok(parsed) => Ok(Self(parsed)),
            Err(e) => {
                tracing::warn!(reason = ?e, value = %s, "failed to parse block number");
                Err(anyhow!("Failed to parse field '{}' with value '{}'", "blockNumber", s))
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
        // HACK: Actually BIGSERIAL, in theory
        // they are equal
        sqlx::postgres::PgTypeInfo::with_name("INT8")
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

impl From<BlockNumber> for u64 {
    fn from(block_number: BlockNumber) -> Self {
        block_number.0.as_u64()
    }
}

impl TryFrom<BlockNumber> for i64 {
    type Error = TryFromIntError;

    fn try_from(block_number: BlockNumber) -> Result<i64, TryFromIntError> {
        i64::try_from(block_number.0.as_u64())
    }
}
