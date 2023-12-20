use crate::eth::evm::EvmInput;
use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CallInput {
    pub from: Address,
    pub to: Address,
    pub data: Bytes,
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<CallInput> for EvmInput {
    fn from(value: CallInput) -> Self {
        Self {
            from: value.from,
            to: Some(value.to),
            data: value.data,
            nonce: None,
        }
    }
}
