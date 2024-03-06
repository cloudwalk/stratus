//! Chain ID Module
//!
//! The Chain ID module provides a unique identifier for different Ethereum
//! networks, such as Mainnet, Ropsten, or Rinkeby. This identifier is crucial
//! in transaction signing to prevent replay attacks across different networks.
//! The module enables the specification and verification of the network for
//! which a particular transaction is intended.

use std::fmt::Display;

use anyhow::anyhow;
use ethereum_types::U256;
use ethereum_types::U64;
use fake::Dummy;
use fake::Faker;

use crate::gen_newtype_from;
use crate::gen_newtype_try_from;

#[derive(Debug, Clone, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChainId(U64);

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
        ChainId(2008.into())
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = ChainId, other = u8, u16, u32, u64);
gen_newtype_try_from!(self = ChainId, other = i32);

impl TryFrom<U256> for ChainId {
    type Error = anyhow::Error;

    fn try_from(value: U256) -> Result<Self, Self::Error> {
        Ok(ChainId(u64::try_from(value).map_err(|err| anyhow!(err))?.into()))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<ChainId> for U256 {
    fn from(value: ChainId) -> Self {
        value.0.as_u64().into()
    }
}
