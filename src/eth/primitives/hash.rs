use std::fmt::Display;

use ethereum_types::H256;

use crate::derive_newtype_from;

#[derive(Debug, Clone, Default, Eq, PartialEq, Hash, serde::Deserialize)]
#[serde(transparent)]
pub struct Hash(H256);

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
// Other -> Self
// -----------------------------------------------------------------------------
derive_newtype_from!(self = Hash, other = H256, [u8; 32]);

// -----------------------------------------------------------------------------
// Self -> Other
// -----------------------------------------------------------------------------
impl From<Hash> for H256 {
    fn from(value: Hash) -> Self {
        value.0
    }
}
