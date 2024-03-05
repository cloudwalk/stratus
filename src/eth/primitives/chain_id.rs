//! Chain ID Module
//!
//! The Chain ID module provides a unique identifier for different Ethereum
//! networks, such as Mainnet, Ropsten, or Rinkeby. This identifier is crucial
//! in transaction signing to prevent replay attacks across different networks.
//! The module enables the specification and verification of the network for
//! which a particular transaction is intended.

use std::fmt::Display;

use ethereum_types::U256;
use fake::Dummy;
use fake::Faker;

use crate::gen_newtype_from;

#[derive(Debug, Clone, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChainId(u64);

impl Display for ChainId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Dummy<Faker> for ChainId {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

impl Default for ChainId {
    fn default() -> Self {
        ChainId(2008)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = ChainId, other = u8, u16, u32, u64, u128, U256, usize, i32);

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<ChainId> for U256 {
    fn from(value: ChainId) -> Self {
        value.0.into()
    }
}
