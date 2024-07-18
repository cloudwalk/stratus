use ethereum_types::U256;

use crate::gen_newtype_from;

// Type representing `r` and `s` variables
// from the ECDSA signature
#[derive(Clone, Copy)]
pub struct EcdsaRs(U256);

impl From<EcdsaRs> for U256 {
    fn from(value: EcdsaRs) -> Self {
        value.0
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
gen_newtype_from!(self = EcdsaRs, other = i64, [u8; 32]);
