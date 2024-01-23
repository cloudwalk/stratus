use ethereum_types::U256;

// Type representing `r` and `s` variables
// from the ECDSA signature 
pub struct Rs(U256);

impl From<Rs> for U256 {
    fn from(value: Rs) -> Self {
        value.0
    }
}
