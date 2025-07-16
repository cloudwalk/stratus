use std::ops::Add;
use std::ops::AddAssign;
use std::str::FromStr;

use alloy_primitives::U64;
use alloy_primitives::U256;
use alloy_primitives::keccak256;
use anyhow::anyhow;
use display_json::DebugAsJson;
use fake::Dummy;
use fake::Faker;

use crate::eth::primitives::Hash;

#[derive(DebugAsJson, derive_more::Display, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct BlockNumber(pub U64);

impl BlockNumber {
    pub const ZERO: BlockNumber = BlockNumber(U64::ZERO);
    pub const ONE: BlockNumber = BlockNumber(U64::ONE);
    pub const MAX: BlockNumber = BlockNumber(U64::MAX);

    /// Calculates the keccak256 hash of the block number.
    pub fn hash(&self) -> Hash {
        Hash::new(*keccak256(<[u8; 8]>::from(*self)))
    }

    /// Returns the previous block number.
    pub fn prev(&self) -> Option<Self> {
        if self.is_zero() { None } else { Some(Self(self.0 - U64::ONE)) }
    }

    /// Returns the next block number.
    pub fn next_block_number(&self) -> Self {
        Self(self.0 + U64::ONE)
    }

    /// Checks if it is the zero block number.
    pub fn is_zero(&self) -> bool {
        self.0.is_zero()
    }

    /// Count how many blocks there is between itself and the othe block.
    ///
    /// Assumes that self is the lower-end of the range.
    pub fn count_to(self, higher_end: BlockNumber) -> u64 {
        if higher_end >= self { higher_end.as_u64() - self.as_u64() + 1 } else { 0 }
    }

    pub fn as_i64(&self) -> i64 {
        self.0.try_into().unwrap()
    }

    pub fn as_u64(&self) -> u64 {
        self.0.try_into().unwrap()
    }

    pub fn as_u32(&self) -> u32 {
        self.0.try_into().unwrap()
    }
}

impl Dummy<Faker> for BlockNumber {
    fn dummy_with_rng<R: rand::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

// -----------------------------------------------------------------------------
// Math
// -----------------------------------------------------------------------------

impl Add<usize> for BlockNumber {
    type Output = BlockNumber;

    fn add(self, rhs: usize) -> Self::Output {
        Self(self.0 + U64::from(rhs))
    }
}

impl AddAssign<usize> for BlockNumber {
    fn add_assign(&mut self, rhs: usize) {
        self.0 = self.0 + U64::from(rhs);
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl From<u8> for BlockNumber {
    fn from(value: u8) -> Self {
        Self(U64::from(value))
    }
}

impl From<u16> for BlockNumber {
    fn from(value: u16) -> Self {
        Self(U64::from(value))
    }
}

impl From<u32> for BlockNumber {
    fn from(value: u32) -> Self {
        Self(U64::from(value))
    }
}

impl From<u64> for BlockNumber {
    fn from(value: u64) -> Self {
        Self(U64::from(value))
    }
}

impl From<U64> for BlockNumber {
    fn from(value: U64) -> Self {
        Self(value)
    }
}

impl From<usize> for BlockNumber {
    fn from(value: usize) -> Self {
        Self(U64::from(value))
    }
}

impl From<i32> for BlockNumber {
    fn from(value: i32) -> Self {
        Self(U64::from(value as u32))
    }
}

impl From<i64> for BlockNumber {
    fn from(value: i64) -> Self {
        Self(U64::from(value as u64))
    }
}

impl FromStr for BlockNumber {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        // This parses a hexadecimal string
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

impl From<BlockNumber> for U256 {
    fn from(block_number: BlockNumber) -> Self {
        Self::from(block_number.as_u64())
    }
}

impl From<BlockNumber> for [u8; 8] {
    fn from(block_number: BlockNumber) -> Self {
        block_number.as_u64().to_be_bytes()
    }
}
