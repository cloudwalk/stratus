use display_json::DebugAsJson;
use ethereum_types::H64;
use fake::Dummy;
use fake::Faker;

use crate::gen_newtype_from;

/// The nonce of an Ethereum block.
#[derive(DebugAsJson, derive_more::Display, Clone, Copy, Default, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct MinerNonce(H64);

impl Dummy<Faker> for MinerNonce {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        H64::random_using(rng).into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = MinerNonce, other = H64, [u8; 8]);

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<MinerNonce> for H64 {
    fn from(value: MinerNonce) -> Self {
        value.0
    }
}

impl From<MinerNonce> for [u8; 8] {
    fn from(value: MinerNonce) -> Self {
        H64::from(value).0
    }
}

// TODO: improve before merging - add to the correct place and simplify imports
impl From<MinerNonce> for alloy_primitives::B64 {
    fn from(value: MinerNonce) -> Self {
        alloy_primitives::B64::new(<[u8; 8]>::from(value))
    }
}
