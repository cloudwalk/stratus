use std::fmt::Display;
use std::ops::Deref;
use std::str::FromStr;

use anyhow::anyhow;
use display_json::DebugAsJson;
use ethabi::Token;
use ethereum_types::H160;
use ethers_core::types::NameOrAddress;
use fake::Dummy;
use fake::Faker;
use hex_literal::hex;
use sqlx::database::HasArguments;
use sqlx::database::HasValueRef;
use sqlx::encode::IsNull;
use sqlx::error::BoxDynError;
use sqlx::postgres::PgHasArrayType;
use sqlx::Decode;

use crate::alias::RevmAddress;
use crate::gen_newtype_from;

/// Address of an Ethereum account (wallet or contract).
#[derive(DebugAsJson, Clone, Copy, Default, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
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

    pub fn new_from_h160(h160: H160) -> Self {
        Self(h160)
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
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
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

impl From<NameOrAddress> for Address {
    fn from(value: NameOrAddress) -> Self {
        match value {
            NameOrAddress::Name(_) => panic!("TODO"),
            NameOrAddress::Address(value) => Self(value),
        }
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
// sqlx traits
// -----------------------------------------------------------------------------
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for Address {
    fn decode(value: <sqlx::Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <[u8; 20] as Decode<sqlx::Postgres>>::decode(value)?;
        Ok(value.into())
    }
}

impl sqlx::Type<sqlx::Postgres> for Address {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("BYTEA")
    }
}

impl PgHasArrayType for Address {
    fn array_type_info() -> sqlx::postgres::PgTypeInfo {
        <[u8; 20] as PgHasArrayType>::array_type_info()
    }
}

impl AsRef<[u8]> for Address {
    fn as_ref(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for Address {
    fn encode_by_ref(&self, buf: &mut <sqlx::Postgres as HasArguments<'q>>::ArgumentBuffer) -> IsNull {
        self.0 .0.encode(buf)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl FromStr for Address {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(H160::from_str(s)?))
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

impl From<Address> for Token {
    fn from(value: Address) -> Self {
        Token::Address(value.0)
    }
}

impl From<Address> for [u8; 20] {
    fn from(value: Address) -> Self {
        H160::from(value).0
    }
}


#[test]
fn test_deref_address() {
    let address = Address::new([0u8; 20]);
    let h160: &H160 = &*address;
    assert_eq!(h160, &H160::zero());
}

#[test]
fn test_display_address() {
    let address = Address::new([0u8; 20]);
    assert_eq!(format!("{}", address), "0x0000000000000000000000000000000000000000");
}

#[test]
fn test_try_from_vec_u8_valid() {
    let bytes = vec![0u8; 20];
    let address = Address::try_from(bytes).unwrap();
    assert_eq!(address.0, H160::zero());
}

#[test]
fn test_from_name_or_address() {
    let name_or_address = NameOrAddress::Address(H160::zero());
    let address: Address = name_or_address.into();
    assert_eq!(address.0, H160::zero());
}

#[test]
fn test_address_new_from_h160() {
    let h160 = H160::zero();
    let address = Address::new_from_h160(h160);
    assert_eq!(address.0, h160);
}

#[test]
fn test_address_new() {
    let bytes = [0u8; 20];
    let address = Address::new(bytes);
    assert_eq!(address.0, H160::zero());
}

#[test]
fn create_address_from_20_byte_array() {
    let bytes = hex!("a9a55a81a4c085ec0c31585aed4cfb09d78dfd53");
    let address = Address::new(bytes);
    assert_eq!(address.0, bytes.into());
}

#[test]
fn create_address_from_invalid_byte_array() {
    let bytes = vec![1, 2, 3, 4, 5];
    let result = Address::try_from(bytes);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().to_string(), "array of bytes to be converted to address must have exactly 20 bytes");
}

#[test]
fn check_if_address_is_coinbase() {
    let coinbase_address = Address::COINBASE;
    let non_coinbase_address = Address::new([1; 20]);

    assert!(coinbase_address.is_coinbase());
    assert!(!non_coinbase_address.is_coinbase());
}

#[test]
fn display_address_as_hex_string() {
    use hex_literal::hex;

    let address = Address::new(hex!("a9a55a81a4c085ec0c31585aed4cfb09d78dfd53"));
    assert_eq!(format!("{}", address), "0xa9a55a81a4c085ec0c31585aed4cfb09d78dfd53");
}

#[test]
fn convert_name_to_address_should_panic() {
    let name = NameOrAddress::Name("John Doe".to_string());
    
    // The conversion from Name to Address should panic
    assert!(std::panic::catch_unwind(|| {
        let _address: Address = name.into();
    }).is_err());
}

#[test]
fn test_is_zero_address() {
    let zero_address = Address::ZERO;
    let non_zero_address = Address::new([1; 20]);

    assert!(zero_address.is_zero());
    assert!(!non_zero_address.is_zero());
}

#[test]
fn test_from_str_valid_address() {
    let address_str = "0x0000000000000000000000000000000000000000";
    let address: Address = address_str.parse().unwrap();
    assert_eq!(address.0, H160::zero());
}

#[test]
fn test_is_ignored_address() {
    let coinbase_address = Address::COINBASE;
    let zero_address = Address::ZERO;
    let non_ignored_address = Address::new([1; 20]);

    assert!(coinbase_address.is_ignored());
    assert!(zero_address.is_ignored());
    assert!(!non_ignored_address.is_ignored());
}

#[test]
fn test_from_str_invalid_address() {
    let address_str = "invalid_address";
    let result: Result<Address, _> = address_str.parse();
    assert!(result.is_err());
}

#[test]
fn test_address_to_h160_conversion() {
    let address = Address::new([1; 20]);
    let h160: H160 = address.into();
    assert_eq!(h160, H160([1; 20]));
}

#[test]
fn test_from_revm_address() {
    // Assuming RevmAddress is a type alias for H160
    let revm_address = H160::zero();
    let address: Address = revm_address.into();
    assert_eq!(address.0, H160::zero());
}

#[test]
fn test_address_to_token_conversion() {
    let address_bytes = hex!("a9a55a81a4c085ec0c31585aed4cfb09d78dfd53");
    let address = Address::new(address_bytes);
    let token: Token = address.into();
        
    assert_eq!(token, Token::Address(H160::from(address_bytes)));
}

#[test]
fn test_address_to_byte_array_conversion() {
    let address_bytes = [1u8; 20];
    let address = Address::new(address_bytes);
    let byte_array: [u8; 20] = address.into();
    assert_eq!(byte_array, address_bytes);
}

#[test]
fn test_address_to_revm_address_conversion() {
    let address_bytes = [1u8; 20];
    let address = Address::new(address_bytes);
    let revm_address: RevmAddress = address.into();
    
    assert_eq!(revm_address, revm::primitives::Address(address_bytes.into()));
}

#[test]
fn test_serde_deserialize() {
    let json = "\"0x0101010101010101010101010101010101010101\"";
    let address: Address = serde_json::from_str(json).unwrap();
    let expected_address = Address::new([1u8; 20]);
    assert_eq!(address, expected_address);
}