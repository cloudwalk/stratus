use display_json::DebugAsJson;
use ethereum_types::U256;
use fake::Dummy;
use fake::Faker;

use crate::gen_newtype_from;

// Type representing `r` and `s` variables from the ECDSA signature.
#[derive(DebugAsJson, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EcdsaRs(U256);

impl Dummy<Faker> for EcdsaRs {
    fn dummy_with_rng<R: rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        Self(U256([rng.next_u64(), rng.next_u64(), rng.next_u64(), rng.next_u64()]))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
gen_newtype_from!(self = EcdsaRs, other = u8, u16, u32, u64, u128, U256, i8, i16, i32, i64, i128, [u8; 32]);
