//! Log Topic Module
//!
//! Handles Ethereum log topics, which are integral to Ethereum's event
//! system. Topics are used for indexing and efficient querying of logs based on
//! event signatures and indexed parameters. This module defines the structure
//! of log topics and provides functionality for handling and converting these
//! identifiers, essential for log filtering and retrieval.

use ethereum_types::H256;
use fake::Dummy;
use fake::Faker;
use revm::primitives::B256 as RevmB256;

/// Topic is part of a [`Log`](super::Log) emitted by the EVM during contract execution.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LogTopic(H256);

impl Dummy<Faker> for LogTopic {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        Self(H256::random_using(rng))
    }
}

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
