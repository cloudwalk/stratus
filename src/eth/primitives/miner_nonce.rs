use alloy_primitives::B64;
use display_json::DebugAsJson;
use fake::Dummy;
use fake::Faker;

use crate::gen_newtype_from;

/// The nonce of an Ethereum block.
#[derive(DebugAsJson, derive_more::Display, Clone, Copy, Default, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct MinerNonce(B64);

impl Dummy<Faker> for MinerNonce {
    fn dummy_with_rng<R: rand::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        B64::random_with(rng).into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = MinerNonce, other = B64, [u8; 8]);

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<MinerNonce> for B64 {
    fn from(value: MinerNonce) -> Self {
        value.0
    }
}

impl From<MinerNonce> for [u8; 8] {
    fn from(value: MinerNonce) -> Self {
        B64::from(value).0
    }
}
