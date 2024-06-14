//! Nonce Module
//!
//! Manages nonces in Ethereum, which are crucial for preventing transaction
//! replay attacks. A nonce is a unique number assigned to each transaction sent
//! by an account, ensuring each transaction is processed once. This module
//! offers functionalities to create, manage, and convert nonces, maintaining
//! the integrity and uniqueness of transactions in the network.

use anyhow::anyhow;
use ethereum_types::U256;
use ethereum_types::U64;
use fake::Dummy;
use fake::Faker;

use crate::gen_newtype_from;
use crate::gen_newtype_try_from;

#[derive(Debug, derive_more::Display, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Nonce(U64);

impl Nonce {
    pub const ZERO: Nonce = Nonce(U64::zero());

    /// Checks if current value is zero.
    pub fn is_zero(&self) -> bool {
        self == &Self::ZERO
    }

    /// Returns the next nonce.
    pub fn next(&self) -> Self {
        Self(self.0 + 1)
    }
}

impl Dummy<Faker> for Nonce {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = Nonce, other = u8, u16, u32, u64);
gen_newtype_try_from!(self = Nonce, other = i32);

impl TryFrom<U256> for Nonce {
    type Error = anyhow::Error;

    fn try_from(value: U256) -> Result<Self, Self::Error> {
        Ok(Nonce(u64::try_from(value).map_err(|err| anyhow!(err))?.into()))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<Nonce> for u64 {
    fn from(value: Nonce) -> Self {
        value.0.as_u64()
    }
}

impl From<Nonce> for U256 {
    fn from(value: Nonce) -> Self {
        U256::from(value.0.as_u64())
    }
}
