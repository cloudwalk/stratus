use display_json::DebugAsJson;
use ethereum_types::U256;

use crate::alias::AlloyUint256;
use crate::gen_newtype_from;

/// A signature component (r or s value)
#[derive(DebugAsJson, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SignatureComponent(pub U256);

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = SignatureComponent, other = U256);

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

impl From<SignatureComponent> for AlloyUint256 {
    fn from(value: SignatureComponent) -> Self {
        // Convert through limbs (internal u64 array) similar to Wei -> RevmU256 conversion
        Self::from_limbs(value.0 .0)
    }
}

impl From<SignatureComponent> for U256 {
    fn from(value: SignatureComponent) -> Self {
        value.0
    }
}
