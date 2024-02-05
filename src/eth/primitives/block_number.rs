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
use std::ops::Add;
use std::ops::AddAssign;
use std::str::FromStr;

use anyhow::anyhow;
use ethereum_types::U64;
use ethers_core::utils::keccak256;
use fake::Dummy;
use fake::Faker;
use sqlx::database::HasValueRef;
use sqlx::error::BoxDynError;

use super::Hash;
use crate::gen_newtype_from;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Add, derive_more::Sub, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct BlockNumber(U64);

impl BlockNumber {
    pub const ZERO: BlockNumber = BlockNumber(U64::zero());

    /// Calculates the keccak256 hash of the block number.
    pub fn hash(&self) -> Hash {
        Hash::new(keccak256(<[u8; 8]>::from(*self)))
    }

    /// Returns the previous block number.
    pub fn prev(&self) -> Option<Self> {
        if self.0.is_zero() {
            None
        } else {
            Some(Self(self.0 - 1))
        }
    }

    /// Returns the next block number.
    pub fn next(&self) -> Self {
        Self(self.0 + 1)
    }

    /// Converts itself to i64.
    pub fn as_i64(&self) -> i64 {
        self.0.as_u64() as i64
    }
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
// Math
// -----------------------------------------------------------------------------

impl Add<usize> for BlockNumber {
    type Output = BlockNumber;

    fn add(self, rhs: usize) -> Self::Output {
        Self(self.0 + rhs)
    }
}

impl AddAssign<usize> for BlockNumber {
    fn add_assign(&mut self, rhs: usize) {
        self.0 = self.0 + rhs;
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

impl From<BlockNumber> for [u8; 8] {
    fn from(block_number: BlockNumber) -> Self {
        block_number.0.as_u64().to_be_bytes()
    }
}

// -----------------------------------------------------------------------------
// Conversions: sqlx
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
