use ethereum_types::H256;
use revm::primitives::B256 as RevmB256;

/// Topic is part of a [`Log`](super::Log) emitted by the EVM during contract execution.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LogTopic(H256);

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
impl AsRef<[u8]> for LogTopic {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl From<RevmB256> for LogTopic {
    fn from(value: RevmB256) -> Self {
        Self(value.0.into())
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<LogTopic> for H256 {
    fn from(value: LogTopic) -> Self {
        value.0
    }
}
