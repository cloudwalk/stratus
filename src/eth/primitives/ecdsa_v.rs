use display_json::DebugAsJson;
use ethereum_types::U64;
use fake::Dummy;
use fake::Faker;

use crate::gen_newtype_from;

// Type representing `v` variable from the ECDSA signature.
#[derive(DebugAsJson, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EcdsaV(U64);

impl Dummy<Faker> for EcdsaV {
    fn dummy_with_rng<R: rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        Self::from(rng.next_u64())
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
gen_newtype_from!(self = EcdsaV, other = u8, u16, u32, u64, U64, i8, i16, i32, [u8; 8]);
