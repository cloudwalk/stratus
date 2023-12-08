use std::fmt::Display;

use ethers_core::types::Bytes as EthersBytes;
use revm::primitives::Bytecode as RevmBytecode;
use revm::primitives::Bytes;

use crate::derive_newtype_from;

/// Bytecode of an EVM contract.
#[derive(Debug, Clone, Default, derive_more::Deref)]
pub struct Bytecode(Vec<u8>);

impl Display for Bytecode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", const_hex::encode(&self.0))
    }
}

impl AsRef<[u8]> for Bytecode {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

// -----------------------------------------------------------------------------
// Other -> Self
// -----------------------------------------------------------------------------
derive_newtype_from!(self = Bytecode, other = Vec<u8>, &[u8]);

impl From<RevmBytecode> for Bytecode {
    fn from(value: RevmBytecode) -> Self {
        Self(value.bytecode.0.into_iter().collect())
    }
}

impl From<EthersBytes> for Bytecode {
    fn from(value: EthersBytes) -> Self {
        Self(value.0.into())
    }
}

// -----------------------------------------------------------------------------
// Self -> Other
// -----------------------------------------------------------------------------
impl From<Bytecode> for Vec<u8> {
    fn from(value: Bytecode) -> Self {
        value.0
    }
}

impl From<Bytecode> for Bytes {
    fn from(value: Bytecode) -> Self {
        value.0.into()
    }
}

impl From<Bytecode> for RevmBytecode {
    fn from(value: Bytecode) -> Self {
        RevmBytecode::new_raw(value.0.into())
    }
}
