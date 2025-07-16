use alloy_primitives::B64;
use display_json::DebugAsJson;
use fake::Dummy;
use fake::Faker;


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

impl From<B64> for MinerNonce {
    fn from(value: B64) -> Self {
        Self(value)
    }
}

impl From<[u8; 8]> for MinerNonce {
    fn from(value: [u8; 8]) -> Self {
        Self(B64::from(value))
    }
}

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
