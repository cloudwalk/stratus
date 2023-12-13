use std::fmt::Display;

use ethereum_types::H256;
use hex_literal::hex;

use crate::derive_newtype_from;

#[derive(Debug, Clone, Default, Eq, PartialEq, Hash, serde::Deserialize)]
#[serde(transparent)]
pub struct Hash(H256);

impl Hash {
    /// Special hash used in block mining to indicate no uncle blocks.
    pub const EMPTY_UNCLE: Hash = Hash::new_const(hex!("1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347"));

    /// Const constructor.
    pub const fn new_const(bytes: [u8; 32]) -> Self {
        Self(H256(bytes))
    }
}

impl Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", const_hex::encode_prefixed(self.0))
    }
}

impl AsRef<[u8]> for Hash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
derive_newtype_from!(self = Hash, other = H256, [u8; 32]);

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<Hash> for H256 {
    fn from(value: Hash) -> Self {
        value.0
    }
}
