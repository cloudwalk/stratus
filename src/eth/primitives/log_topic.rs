use std::fmt::Display;

use alloy_primitives::FixedBytes;
use alloy_primitives::B256;
use display_json::DebugAsJson;
use fake::Dummy;
use fake::Faker;

use crate::alias::RevmB256;
use crate::gen_newtype_from;

/// Topic is part of a [`Log`](super::Log) emitted by the EVM during contract execution.
#[derive(DebugAsJson, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default, Hash)]
pub struct LogTopic(pub B256);

impl Display for LogTopic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", const_hex::encode_prefixed(self.0))
    }
}

impl Dummy<Faker> for LogTopic {
    fn dummy_with_rng<R: rand_core::RngCore + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        Self(FixedBytes::random_with(rng))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = LogTopic, other = FixedBytes<32>, [u8; 32]);

impl From<RevmB256> for LogTopic {
    fn from(value: RevmB256) -> Self {
        Self(value.0.into())
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
        value.0 .0
    }
}

impl From<LogTopic> for B256 {
    fn from(value: LogTopic) -> Self {
        Self::from(value.0)
    }
}
