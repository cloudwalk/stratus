use std::fmt::Display;

use revm::primitives::Bytecode as RevmBytecode;
use revm::primitives::Bytes;

/// Bytecode of an EVM contract.
#[derive(Debug, Clone, Default, derive_more::From, derive_more::Deref)]
pub struct Bytecode(Vec<u8>);

impl Display for Bytecode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", const_hex::encode(&self.0))
    }
}

impl From<RevmBytecode> for Bytecode {
    fn from(value: RevmBytecode) -> Self {
        value.bytecode.0.to_vec().into()
    }
}

impl From<&[u8]> for Bytecode {
    fn from(value: &[u8]) -> Self {
        Self(value.to_vec())
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
