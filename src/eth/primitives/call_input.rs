use display_json::DebugAsJson;
use serde::Deserialize;

use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::Wei;

#[derive(DebugAsJson, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize)]
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

impl<'de> Deserialize<'de> for CallInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = serde_json::Value::deserialize(deserializer)?;

        let from = serde_json::from_value(s.get("from").cloned().unwrap_or(serde_json::Value::Null)).map_err(serde::de::Error::custom)?;
        let to = serde_json::from_value(s.get("to").cloned().unwrap_or(serde_json::Value::Null)).map_err(serde::de::Error::custom)?;
        let value = serde_json::from_value(s.get("value").cloned().unwrap_or_else(|| serde_json::Value::String("0x0".to_string())))
            .map_err(serde::de::Error::custom)?;

        let data_value = s.get("data").or_else(|| s.get("input")).cloned().unwrap_or(serde_json::Value::Null);
        let data = serde_json::from_value(data_value).unwrap_or_default();

        Ok(CallInput { from, to, value, data })
    }
}
