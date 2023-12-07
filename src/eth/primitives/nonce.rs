use std::fmt::Display;

use ethereum_types::U64;

use crate::derive_newtype_from;

#[derive(Debug, Clone, Default)]
pub struct Nonce(U64);

impl Display for Nonce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// -----------------------------------------------------------------------------
// Other -> Self
// -----------------------------------------------------------------------------
derive_newtype_from!(self = Nonce, other = u8, u16, u32, u64, usize);

// -----------------------------------------------------------------------------
// Self -> Other
// -----------------------------------------------------------------------------
impl From<Nonce> for usize {
    fn from(value: Nonce) -> Self {
        value.0.as_usize()
    }
}

impl From<Nonce> for u64 {
    fn from(value: Nonce) -> Self {
        value.0.as_u64()
    }
}
