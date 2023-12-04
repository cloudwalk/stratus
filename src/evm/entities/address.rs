use std::fmt::Display;

use ethabi::Token;
use hex_literal::hex;
use primitive_types::H160;
use revm::primitives::Address as RevmAddress;

/// Address of an EVM account (wallet or contract).
#[derive(Debug, Clone, Default, Eq, PartialEq, Hash, derive_more::From)]
pub struct Address(pub H160);

impl Address {
    // Special ETH address used in several
    pub const ZERO: Address = Address(H160::zero());

    /// Special address that receives the block reward.
    pub const COINBASE: Address = Address(H160(hex!("00000000000000000000000000000000000000ff")));

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

impl From<[u8; 20]> for Address {
    fn from(value: [u8; 20]) -> Self {
        Address(H160(value))
    }
}

impl From<RevmAddress> for Address {
    fn from(value: RevmAddress) -> Self {
        Address(value.0 .0.into())
    }
}

impl From<Address> for RevmAddress {
    fn from(value: Address) -> Self {
        Self(value.0 .0.into())
    }
}

impl From<Address> for Token {
    fn from(value: Address) -> Self {
        Self::Address(value.0)
    }
}
