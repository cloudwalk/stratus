use ethereum_types::H256;

use crate::derive_newtype_from;

pub struct Hash(H256);

// -----------------------------------------------------------------------------
// Other -> Self
// -----------------------------------------------------------------------------
derive_newtype_from!(self = Hash, other = H256, [u8; 32]);
