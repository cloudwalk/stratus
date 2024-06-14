use ethereum_types::U64;

use crate::gen_newtype_from;

// Type representing `v` variable
// from the ECDSA signature
#[derive(Clone, Copy)]
pub struct EcdsaV(U64);

impl From<EcdsaV> for U64 {
    fn from(value: EcdsaV) -> Self {
        value.0
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
gen_newtype_from!(self = EcdsaV, other = i32, [u8; 8]);
