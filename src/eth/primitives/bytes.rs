use std::fmt::Display;
use std::ops::Deref;
use std::ops::DerefMut;

use display_json::DebugAsJson;

use crate::alias::RevmBytes;
use crate::alias::RevmOutput;

#[derive(DebugAsJson, Clone, Default, Eq, PartialEq, fake::Dummy)]
pub struct Bytes(pub Vec<u8>);

impl Display for Bytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.len() <= 256 {
            write!(f, "{}", const_hex::encode_prefixed(&self.0))
        } else {
            write!(f, "too long")
        }
    }
}

// -----------------------------------------------------------------------------
// Serialization / Deserialization
// -----------------------------------------------------------------------------
impl serde::Serialize for Bytes {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&const_hex::encode_prefixed(&self.0))
    }
}

impl<'de> serde::Deserialize<'de> for Bytes {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match const_hex::decode(value) {
            Ok(value) => Ok(Self(value)),
            Err(e) => {
                tracing::warn!(reason = ?e, "failed to parse hex bytes");
                Err(serde::de::Error::custom(e))
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl From<Vec<u8>> for Bytes {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}

impl From<&[u8]> for Bytes {
    fn from(value: &[u8]) -> Self {
        Self(value.to_vec())
    }
}

impl From<[u8; 32]> for Bytes {
    fn from(value: [u8; 32]) -> Self {
        Self(value.to_vec())
    }
}

impl From<RevmBytes> for Bytes {
    fn from(value: RevmBytes) -> Self {
        Self(value.0.into())
    }
}

impl From<RevmOutput> for Bytes {
    fn from(value: RevmOutput) -> Self {
        match value {
            RevmOutput::Call(bytes) => Self(bytes.0.into()),
            RevmOutput::Create(bytes, _) => Self(bytes.0.into()),
        }
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Deref for Bytes {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Bytes {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<Bytes> for RevmBytes {
    fn from(value: Bytes) -> Self {
        value.0.into()
    }
}
