use std::fmt::Display;

use alloy_primitives::B256;
use alloy_primitives::FixedBytes;
use display_json::DebugAsJson;
use fake::Dummy;
use fake::Faker;

/// Topic is part of a [`Log`](super::Log) emitted by the EVM during contract execution.
#[derive(DebugAsJson, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default, Hash)]
pub struct LogTopic(pub B256);

impl Display for LogTopic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", const_hex::encode_prefixed(self.0))
    }
}

impl Dummy<Faker> for LogTopic {
    fn dummy_with_rng<R: rand::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        Self(FixedBytes::random_with(rng))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl From<FixedBytes<32>> for LogTopic {
    fn from(value: FixedBytes<32>) -> Self {
        Self(value)
    }
}

impl From<[u8; 32]> for LogTopic {
    fn from(value: [u8; 32]) -> Self {
        Self(FixedBytes::from(value))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl AsRef<[u8]> for LogTopic {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl From<LogTopic> for [u8; 32] {
    fn from(value: LogTopic) -> Self {
        value.0.0
    }
}

impl From<LogTopic> for B256 {
    fn from(value: LogTopic) -> Self {
        value.0
    }
}
