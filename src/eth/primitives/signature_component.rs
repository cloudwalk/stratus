use alloy_primitives::Uint;
use display_json::DebugAsJson;
use ethereum_types::U256;
use fake::Dummy;
use fake::Faker;
use rand::Rng;

use crate::alias::AlloyUint256;
use crate::gen_newtype_from;

/// A signature component (r or s value)
#[derive(DebugAsJson, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SignatureComponent(pub U256);

impl Dummy<Faker> for SignatureComponent {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        Self(U256::from(rng.gen::<u64>()))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = SignatureComponent, other = U256);

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<SignatureComponent> for AlloyUint256 {
    fn from(value: SignatureComponent) -> Self {
        Self::from_limbs(value.0 .0)
    }
}

impl From<SignatureComponent> for U256 {
    fn from(value: SignatureComponent) -> Self {
        value.0
    }
}

impl From<Uint<256, 4>> for SignatureComponent {
    fn from(value: Uint<256, 4>) -> Self {
        Self(U256::from(value.to_be_bytes::<32>()))
    }
}
