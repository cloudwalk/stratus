use display_json::DebugAsJson;
use serde::Deserialize;

use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::Wei;

#[derive(serde::Deserialize)]
struct CallInputDataField {
    data: Option<Bytes>,
    input: Option<Bytes>,
}

fn deserialize_data_field<'d, D: serde::Deserializer<'d>>(d: D) -> Result<Bytes, D::Error> {
    let CallInputDataField { data, input } = CallInputDataField::deserialize(d)?;
    Ok(data.or(input).unwrap_or_default())
}

#[derive(DebugAsJson, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct CallInput {
    #[serde(rename = "from")]
    pub from: Option<Address>,

    #[serde(rename = "to")]
    pub to: Option<Address>,

    #[serde(rename = "value", default)]
    pub value: Wei,

    #[serde(deserialize_with = "deserialize_data_field", flatten, default)]
    pub data: Bytes,
}
