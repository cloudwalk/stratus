use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CallInput {
    pub from: Address,
    pub to: Option<Address>,
    #[serde(default)]
    pub data: Bytes,
}
