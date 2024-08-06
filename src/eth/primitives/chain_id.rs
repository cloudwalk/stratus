use crate::gen_newtype_from;
use crate::gen_newtype_try_from;
use anyhow::anyhow;
use display_json::DebugAsJson;
use ethereum_types::U256;
use ethereum_types::U64;
use fake::Dummy;
use fake::Faker;

#[derive(DebugAsJson, derive_more::Display, Clone, Copy, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChainId(pub U64);

impl ChainId {
    pub fn new(value: U64) -> Self {
        Self(value)
    }
}

impl Dummy<Faker> for ChainId {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = ChainId, other = u8, u16, u32, u64);
gen_newtype_try_from!(self = ChainId, other = i32);

impl TryFrom<U256> for ChainId {
    type Error = anyhow::Error;

    fn try_from(value: U256) -> Result<Self, Self::Error> {
        Ok(ChainId(u64::try_from(value).map_err(|err| anyhow!(err))?.into()))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<ChainId> for u64 {
    fn from(value: ChainId) -> Self {
        value.0.as_u64()
    }
}

impl From<ChainId> for U256 {
    fn from(value: ChainId) -> Self {
        value.0.as_u64().into()
    }
}
// Creating a new ChainId instance with a valid U64 value
#[test]
fn create_chainid_with_valid_u64() {
    use ethereum_types::U64;
    let value = U64::from(42);
    let chain_id = ChainId::new(value);
    assert_eq!(chain_id.0, value);
}
#[test]
fn conversion_from_u256_to_chainid_too_large() {
    use ethereum_types::U256;
    let value = U256::from(u64::MAX) + 1;
    let result = ChainId::try_from(value);
    assert!(result.is_err());
}

// Handling conversion from i32 to ChainId when i32 value is negative
#[test]
#[should_panic]
fn handle_conversion_from_negative_i32() {
    let negative_value = -42;
    let chain_id = ChainId::try_from(negative_value);
    assert!(chain_id.is_err());
}
// Displaying a ChainId instance using the Display trait
#[test]
fn display_chainid_instance() {
    use std::fmt::Write;
    let chain_id = ChainId::new(U64::from(100));
    let mut output = String::new();
    write!(&mut output, "{}", chain_id).unwrap();
    assert_eq!(output, "100");
}
// Converting u8, u16, u32, and u64 to ChainId using gen_newtype_from
#[test]
fn convert_u8_u16_u32_u64_to_chainid() {
    let value_u8: u8 = 10;
    let chain_id_u8 = ChainId::from(value_u8);
    assert_eq!(chain_id_u8.0.as_u64(), u64::from(value_u8));

    let value_u16: u16 = 100;
    let chain_id_u16 = ChainId::from(value_u16);
    assert_eq!(chain_id_u16.0.as_u64(), u64::from(value_u16));

    let value_u32: u32 = 1000;
    let chain_id_u32 = ChainId::from(value_u32);
    assert_eq!(chain_id_u32.0.as_u64(), u64::from(value_u32));

    let value_u64: u64 = 10000;
    let chain_id_u64 = ChainId::from(value_u64);
    assert_eq!(chain_id_u64.0.as_u64(), value_u64);
}
// Converting a ChainId instance to u64
#[test]
fn test_converting_chainid_to_u64() {
    let u64_value = 42;
    let chain_id = ChainId::new(U64::from(u64_value));

    let converted_u64: u64 = chain_id.into();

    assert_eq!(converted_u64, u64_value);
}
#[test]
// Creating a ChainId instance with a U64 value of zero
fn test_creating_chainid_with_zero_value() {
    let zero_value = 0;
    let chain_id = ChainId::new(U64::from(zero_value));

    assert_eq!(chain_id.0.as_u64(), zero_value);
}
// Cloning and copying a ChainId instance
#[test]
fn clone_and_copy_chainid_instance() {
    let value = U64::from(42);
    let chain_id = ChainId::new(value);

    // Cloning the ChainId instance&chain_id.debug_as_json
    let cloned_chain_id = chain_id.clone();

    // Copying the ChainId instance
    let copied_chain_id = chain_id;

    assert_eq!(chain_id.0, value);
    assert_eq!(cloned_chain_id.0, value);
    assert_eq!(copied_chain_id.0, value);
}
// Comparing two ChainId instances for equality
#[test]
fn compare_two_chainid_instances_for_equality() {
    let chain_id1 = ChainId::new(U64::from(100));
    let chain_id2 = ChainId::new(U64::from(100));

    assert_eq!(chain_id1, chain_id2);
}
// Converting a ChainId instance to U256
#[test]
fn convert_chainid_to_u256() {
    use ethereum_types::U256;
    use std::convert::TryFrom;

    let u64_value = 100u64;
    let chain_id = ChainId::new(u64_value.into());
    let u256_value = U256::try_from(chain_id).unwrap();

    assert_eq!(u256_value, U256::from(u64_value));
}
#[test]
fn test_converting_chainid_with_zero_value() {
    let chain_id = ChainId::new(0.into());

    let u64_value: u64 = chain_id.into();
    let u256_value: U256 = chain_id.into();

    assert_eq!(u64_value, 0);
    assert_eq!(u256_value, U256::zero());
}
// Handling errors during serialization and deserialization
#[test]
fn handle_errors_during_serialization_and_deserialization() {
    use ethereum_types::U256;
    use std::convert::TryFrom;

    let valid_u256 = U256::from(100);
    let chain_id_result = ChainId::try_from(valid_u256);

    assert!(chain_id_result.is_ok());

    let invalid_u256 = U256::MAX;
    let chain_id_error = ChainId::try_from(invalid_u256);

    assert!(chain_id_error.is_err());
}
