use ethereum_types::H256;

use crate::derive_newtype_from;

#[derive(Debug, Clone, Default, Eq, PartialEq, Hash)]
pub struct Hash(H256);

impl AsRef<[u8]> for Hash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

// -----------------------------------------------------------------------------
// Other -> Self
// -----------------------------------------------------------------------------
derive_newtype_from!(self = Hash, other = H256, [u8; 32]);
