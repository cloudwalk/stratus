use alloy_primitives::U256;
use anyhow::Context;
use display_json::DebugAsJson;
use fake::Dummy;
use fake::Faker;

use crate::gen_newtype_from;

/// Native token amount in wei.
#[derive(DebugAsJson, derive_more::Display, Clone, Copy, Default, PartialOrd, Ord, PartialEq, Eq, derive_more::Sub, serde::Serialize, serde::Deserialize)]
pub struct Wei(pub U256);

impl Wei {
    pub const ZERO: Wei = Wei(U256::ZERO);
    pub const ONE: Wei = Wei(U256::ONE);
    pub const TEST_BALANCE: Wei = Wei(U256::from_limbs([u64::MAX, 0, 0, 0]));

    /// Converts a hexadecimal string to Wei
    ///
    /// # Arguments
    ///
    /// * `hex` - A hexadecimal string, with or without the "0x" prefix
    ///
    /// # Returns
    ///
    /// A Result containing the Wei value or an error
    ///
    /// # Examples
    ///
    /// ```
    /// use stratus::eth::primitives::Wei;
    ///
    /// let wei = Wei::from_hex_str("1a").unwrap();
    /// assert_eq!(wei, Wei::from(26u64));
    ///
    /// let wei = Wei::from_hex_str("0x1a").unwrap();
    /// assert_eq!(wei, Wei::from(26u64));
    /// ```
    pub fn from_hex_str(hex: &str) -> Result<Self, anyhow::Error> {
        let hex = hex.trim_start_matches("0x");
        let u256 = U256::from_str_radix(hex, 16)?;
        Ok(Self(u256))
    }

    /// Alias for from_hex_str for backward compatibility
    #[deprecated(since = "0.20.1", note = "Use from_hex_str instead")]
    pub fn from_str_hex(hex: &str) -> Result<Self, anyhow::Error> {
        Self::from_hex_str(hex)
    }
}

impl Dummy<Faker> for Wei {
    fn dummy_with_rng<R: rand::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = Wei, other = u8, u16, u32, u64, u128, U256, usize, i32);

impl From<[u64; 4]> for Wei {
    fn from(value: [u64; 4]) -> Self {
        Self(U256::from_limbs(value))
    }
}

// impl TryFrom<BigDecimal> for Wei {
//     type Error = anyhow::Error;

//     fn try_from(value: BigDecimal) -> Result<Self, Self::Error> {
//         let value_str = value.to_string();
//         Ok(Wei(U256::from_dec_str(&value_str)?))
//     }
// }

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

impl TryFrom<Wei> for u128 {
    type Error = anyhow::Error;

    fn try_from(value: Wei) -> Result<Self, Self::Error> {
        u128::try_from(value.0).context("wei conversion failed")
    }
}

impl From<Wei> for U256 {
    fn from(value: Wei) -> Self {
        value.0
    }
}

// impl TryFrom<Wei> for BigDecimal {
//     type Error = anyhow::Error;
//     fn try_from(value: Wei) -> Result<Self, Self::Error> {
//         // HACK: If we could import BigInt or BigUint we could convert the bytes directly.
//         Ok(BigDecimal::from_str(&U256::from(value).to_string())?)
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;

    // #[test]
    // fn big_decimal_to_nonce_conversion() {
    //     // Test with a simple value
    //     let big_decimal = BigDecimal::new(1.into(), -4);
    //     let nonce: Wei = big_decimal.clone().try_into().unwrap();
    //     let expected = nonce.0.as_u64();
    //     assert_eq!(10000, expected);
    // }

    #[test]
    fn test_from_hex_str() {
        // Test with a simple value without 0x prefix
        let wei = Wei::from_hex_str("1a").unwrap();
        assert_eq!(wei, Wei::from(26u64));

        // Test with 0x prefix
        let wei = Wei::from_hex_str("0x1a").unwrap();
        assert_eq!(wei, Wei::from(26u64));

        // Test with a larger value
        let wei = Wei::from_hex_str("0xffff").unwrap();
        assert_eq!(wei, Wei::from(65535u64));
    }
}
