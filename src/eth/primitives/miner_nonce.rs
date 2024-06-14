use ethereum_types::H64;
use fake::Dummy;
use fake::Faker;

use crate::gen_newtype_from;

/// The nonce of an Ethereum block.
#[derive(Debug, derive_more::Display, Clone, Copy, Default, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct MinerNonce(H64);

impl MinerNonce {
    /// Creates a new BlockNonce from the given bytes.
    pub const fn new(bytes: [u8; 8]) -> Self {
        Self(H64(bytes))
    }
}

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
