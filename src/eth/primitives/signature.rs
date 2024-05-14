use std::borrow::Cow;

use crate::eth::codegen;

/// Alias for 4 byte signature used to identify functions and errors.
pub type Signature4Bytes = [u8; 4];

/// Alias for 32 byte signature used to identify events.
pub type Signature32Bytes = [u8; 32];

/// Alias for a Solidity function, error or event signature.
pub type SoliditySignature = Cow<'static, str>;

pub enum Signature {
    Function(Signature4Bytes),
    Event(Signature32Bytes),
}

impl Signature {
    pub fn extract(self) -> SoliditySignature {
        match self {
            Signature::Function(id) => match codegen::SIGNATURES_4_BYTES.get(&id) {
                Some(signature) => Cow::from(*signature),
                None => Cow::from(const_hex::encode_prefixed(id)),
            },
            Signature::Event(id) => match codegen::SIGNATURES_32_BYTES.get(&id) {
                Some(signature) => Cow::from(*signature),
                None => Cow::from(const_hex::encode_prefixed(id)),
            },
        }
    }
}
