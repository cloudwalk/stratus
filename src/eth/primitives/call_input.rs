use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::Wei;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CallInput {
    pub from: Address,
    pub to: Option<Address>,

    #[serde(default)]
    pub value: Wei,

    #[serde(default)]
    pub data: Bytes,
}
