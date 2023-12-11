use std::fmt::Display;

use ethabi::Token;
use ethereum_types::H160;
use ethers_core::types::NameOrAddress;
use hex_literal::hex;
use revm::primitives::Address as RevmAddress;

use crate::derive_newtype_from;

/// Address of an Ethereum account (wallet or contract).
#[derive(Debug, Clone, Default, Eq, PartialEq, Hash, serde::Deserialize)]
pub struct Address(H160);

impl Address {
    // Special ETH address used in several
    pub const ZERO: Address = Address(H160::zero());

    /// Special address that receives the block reward.
    pub const COINBASE: Address = Address(H160(hex!("00000000000000000000000000000000000000ff")));

    /// Const constructor.
    pub const fn from_bytes(bytes: [u8; 20]) -> Self {
        Self(H160(bytes))
    }

    /// Checks if current address is the zero address.
    pub fn is_zero(&self) -> bool {
        self == &Self::ZERO
    }

    /// Checks if current address is the coinbase address.
    pub fn is_coinbase(&self) -> bool {
        self == &Self::COINBASE
    }
}

impl Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#x}", self.0)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
derive_newtype_from!(self = Address, other = H160, [u8; 20]);

impl From<RevmAddress> for Address {
    fn from(value: RevmAddress) -> Self {
        Address(value.0 .0.into())
    }
}

impl From<NameOrAddress> for Address {
    fn from(value: NameOrAddress) -> Self {
        match value {
            NameOrAddress::Name(_) => panic!("TODO"),
            NameOrAddress::Address(value) => Self(value),
        }
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<Address> for H160 {
    fn from(value: Address) -> Self {
        value.0
    }
}

impl From<Address> for RevmAddress {
    fn from(value: Address) -> Self {
        RevmAddress(value.0 .0.into())
    }
}

impl From<Address> for Token {
    fn from(value: Address) -> Self {
        Token::Address(value.0)
    }
}
