use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CallInput {
    pub from: Address,
    pub to: Address,
    pub data: Bytes,
}
