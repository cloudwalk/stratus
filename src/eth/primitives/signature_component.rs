use alloy_primitives::U256;
use display_json::DebugAsJson;
use fake::Dummy;
use fake::Faker;
use rand::Rng;


/// A signature component (r or s value)
#[derive(DebugAsJson, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SignatureComponent(pub U256);

impl Dummy<Faker> for SignatureComponent {
    fn dummy_with_rng<R: Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        Self(U256::random_with(rng))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl From<U256> for SignatureComponent {
    fn from(value: U256) -> Self {
        Self(value)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

impl From<SignatureComponent> for U256 {
    fn from(value: SignatureComponent) -> Self {
        value.0
    }
}
