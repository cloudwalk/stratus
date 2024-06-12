use prost::Message;
use serde::Deserialize;
use serde::Serialize;

use super::append_entry::BlockHeader as BH;
use super::append_entry::TransactionExecution as TE;

#[allow(clippy::large_enum_variant)]
#[derive(Serialize, Deserialize, Debug, Clone)]
enum LogEntryData {
    BlockHeader(BH),
    TransactionExecution(TE),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct LogEntry {
    index: u64,
    term: u64,
    data: LogEntryData,
}

// Implement Serialize and Deserialize for BH
impl Serialize for BH {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let bytes = self.encode_to_vec();
        serializer.serialize_bytes(&bytes)
    }
}

impl<'de> Deserialize<'de> for BH {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let bytes: Vec<u8> = serde::Deserialize::deserialize(deserializer)?;
        BH::decode(&*bytes).map_err(serde::de::Error::custom)
    }
}

// Implement Serialize and Deserialize for TE
impl Serialize for TE {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let bytes = self.encode_to_vec();
        serializer.serialize_bytes(&bytes)
    }
}

impl<'de> Deserialize<'de> for TE {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let bytes: Vec<u8> = serde::Deserialize::deserialize(deserializer)?;
        TE::decode(&*bytes).map_err(serde::de::Error::custom)
    }
}
