use std::fmt::Display;

use display_json::DebugAsJson;
use ethereum_types::H256;
use fake::Dummy;
use fake::Faker;

use crate::alias::RevmB256;
use crate::gen_newtype_from;

/// Topic is part of a [`Log`](super::Log) emitted by the EVM during contract execution.
#[derive(DebugAsJson, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default, Hash)]
pub struct LogTopic(pub H256);

impl LogTopic {
    pub fn new(inner: H256) -> Self {
        Self(inner)
    }
}

impl Display for LogTopic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", const_hex::encode_prefixed(self.0))
    }
}

impl Dummy<Faker> for LogTopic {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        Self(H256::random_using(rng))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = LogTopic, other = H256, [u8; 32]);

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

impl From<LogTopic> for H256 {
    fn from(value: LogTopic) -> Self {
        value.0
    }
}
