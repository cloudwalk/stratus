use display_json::DebugAsJson;

use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::Wei;

#[derive(DebugAsJson, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct CallInput {
    #[serde(rename = "from")]
    pub from: Option<Address>,

    #[serde(rename = "to")]
    pub to: Option<Address>,

    #[serde(rename = "value", default)]
    pub value: Wei,

    #[serde(rename = "data", alias = "input", default)]
    pub data: Bytes,
}
