use std::fmt::Display;

use ethabi::Token;
use ethereum_types::U256;
use revm::primitives::U256 as RevmU256;

use crate::derive_newtype_from;

/// Native token amount in wei.
#[derive(Debug, Clone, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Wei(U256);

impl Wei {
    pub const ZERO: Wei = Wei(U256::zero());
    pub const ONE: Wei = Wei(U256::one());
    pub const MAX: Wei = Wei(U256::max_value());
}

impl Display for Wei {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
derive_newtype_from!(self = Wei, other = U256, u8, u16, u32, u64, u128, usize);

impl From<RevmU256> for Wei {
    fn from(value: RevmU256) -> Self {
        Self(value.to_be_bytes().into())
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<Wei> for Token {
    fn from(value: Wei) -> Self {
        Token::Uint(value.0)
    }
}

impl From<Wei> for RevmU256 {
    fn from(value: Wei) -> Self {
        RevmU256::from_limbs(value.0 .0)
    }
}

impl From<Wei> for U256 {
    fn from(value: Wei) -> Self {
        value.0
    }
}
