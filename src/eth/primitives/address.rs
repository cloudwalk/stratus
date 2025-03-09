use std::fmt::Display;
use std::ops::Deref;
use std::str::FromStr;

use alloy_primitives::hex as alloy_hex;
use anyhow::anyhow;
use display_json::DebugAsJson;
use ethereum_types::H160;
use ethereum_types::H256;
use fake::Dummy;
use fake::Faker;
use hex_literal::hex;

use crate::alias::RevmAddress;
use crate::eth::primitives::LogTopic;
use crate::gen_newtype_from;

/// Address of an Ethereum account (wallet or contract).
#[derive(DebugAsJson, Clone, Copy, Default, Eq, PartialEq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub struct Address(pub H160);

impl Address {
    // Special ETH address used in some contexts.
    pub const ZERO: Address = Address(H160::zero());

    /// Special address that receives the block reward.
    pub const COINBASE: Address = Address(H160(hex!("00000000000000000000000000000000000000ff")));
    pub const BRLC: Address = Address(H160(hex!("a9a55a81a4c085ec0c31585aed4cfb09d78dfd53")));

    /// Creates a new address from the given bytes.
    pub const fn new(bytes: [u8; 20]) -> Self {
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

    /// Checks if current address should have their updates ignored.
    ///
    /// * Coinbase is ignored because we do not charge gas, otherwise it will have to be updated for every transaction.
    /// * Not sure if zero address should be ignored or not.
    pub fn is_ignored(&self) -> bool {
        self.is_coinbase() || self.is_zero()
    }
}

impl Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", const_hex::encode_prefixed(self.0))
    }
}

impl Dummy<Faker> for Address {
    fn dummy_with_rng<R: rand_core::RngCore + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        H160::random_using(rng).into()
    }
}

impl Deref for Address {
    type Target = H160;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = Address, other = H160, [u8; 20]);

impl From<RevmAddress> for Address {
    fn from(value: RevmAddress) -> Self {
        Address(value.0 .0.into())
    }
}

impl From<LogTopic> for Address {
    fn from(value: LogTopic) -> Self {
        Self(H160::from_slice(&value.0 .0[12..32]))
    }
}

impl TryFrom<Vec<u8>> for Address {
    type Error = anyhow::Error;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        if value.len() != 20 {
            return Err(anyhow!("array of bytes to be converted to address must have exactly 20 bytes"));
        }
        Ok(Self(H160::from_slice(&value)))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
/// Converts a hexadecimal string representation to an Address.
///
/// The input string can be with or without the "0x" prefix.
/// If the string has an odd number of digits, a leading zero will be added.
///
/// # Examples
///
/// ```
/// use std::str::FromStr;
/// use stratus::eth::primitives::Address;
///
/// // With 0x prefix
/// let addr1 = Address::from_str("0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266").unwrap();
///
/// // Without 0x prefix
/// let addr2 = Address::from_str("f39fd6e51aad88f6f4ce6ab8827279cfffb92266").unwrap();
///
/// assert_eq!(addr1, addr2);
/// ```
impl FromStr for Address {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Remove 0x prefix if present
        let s = s.trim_start_matches("0x");

        // Ensure the hex string has an even number of digits
        let s = if s.len() % 2 == 1 {
            format!("0{}", s) // Add a leading zero if needed
        } else {
            s.to_string()
        };

        // Validate length
        if s.len() != 40 {
            return Err(anyhow!("Invalid address length: {}", s.len()));
        }

        // Decode hex string using alloy_hex instead of hex
        let bytes = alloy_hex::decode(&s)?;

        // Create address from bytes
        let mut array = [0u8; 20];
        array.copy_from_slice(&bytes);
        Ok(Address::new(array))
    }
}

impl From<Address> for H160 {
    fn from(value: Address) -> Self {
        value.0
    }
}

impl From<Address> for RevmAddress {
    fn from(value: Address) -> Self {
        revm::primitives::Address(value.0 .0.into())
    }
}

impl From<Address> for LogTopic {
    fn from(value: Address) -> Self {
        Self(H256::from(value.0))
    }
}
