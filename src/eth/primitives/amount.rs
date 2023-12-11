use std::fmt::Display;

use ethabi::Token;
use ethereum_types::U256;
use revm::primitives::U256 as RevmU256;

use crate::derive_newtype_from;

/// Native token amount (represented in wei).
#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct Amount(U256);

impl Amount {
    pub const ZERO: Amount = Amount(U256::zero());
    pub const ONE: Amount = Amount(U256::one());
}

impl Display for Amount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
derive_newtype_from!(self = Amount, other = U256, u8, u16, u32, u64, u128, usize);

impl From<RevmU256> for Amount {
    fn from(value: RevmU256) -> Self {
        Self(value.to_be_bytes().into())
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<Amount> for Token {
    fn from(value: Amount) -> Self {
        Token::Uint(value.0)
    }
}

impl From<Amount> for RevmU256 {
    fn from(value: Amount) -> Self {
        RevmU256::from_limbs(value.0 .0)
    }
}
