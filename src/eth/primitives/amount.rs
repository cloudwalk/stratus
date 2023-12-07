use ethabi::Token;
use ethereum_types::U256;

use crate::derive_newtype_from;

#[derive(Debug, Clone, Default)]
pub struct Amount(U256);

impl Amount {
    pub const ZERO: Amount = Amount(U256::zero());
    pub const ONE: Amount = Amount(U256::one());
}

// -----------------------------------------------------------------------------
// Other -> Self
// -----------------------------------------------------------------------------
derive_newtype_from!(self = Amount, other = U256, u8, u16, u32, u64, u128, usize);

// -----------------------------------------------------------------------------
// Self -> Otherclear
// -----------------------------------------------------------------------------
impl From<Amount> for Token {
    fn from(value: Amount) -> Self {
        Token::Uint(value.0)
    }
}
