//! Log Topic Module
//!
//! Handles Ethereum log topics, which are integral to Ethereum's event
//! system. Topics are used for indexing and efficient querying of logs based on
//! event signatures and indexed parameters. This module defines the structure
//! of log topics and provides functionality for handling and converting these
//! identifiers, essential for log filtering and retrieval.

use std::fmt::Display;

use ethereum_types::H256;
use fake::Dummy;
use fake::Faker;
use revm::primitives::B256 as RevmB256;
use sqlx::postgres::PgHasArrayType;

use crate::gen_newtype_from;

/// Topic is part of a [`Log`](super::Log) emitted by the EVM during contract execution.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LogTopic(H256);

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
gen_newtype_from!(self = LogTopic, other = H256);

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

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for LogTopic {
    fn encode(self, buf: &mut <sqlx::Postgres as sqlx::database::HasArguments<'q>>::ArgumentBuffer) -> sqlx::encode::IsNull
    where
        Self: Sized,
    {
        <&[u8; 32] as sqlx::Encode<sqlx::Postgres>>::encode(self.0.as_fixed_bytes(), buf)
    }

    fn encode_by_ref(&self, buf: &mut <sqlx::Postgres as sqlx::database::HasArguments<'q>>::ArgumentBuffer) -> sqlx::encode::IsNull {
        <&[u8; 32] as sqlx::Encode<sqlx::Postgres>>::encode(self.0.as_fixed_bytes(), buf)
    }
}

impl sqlx::Type<sqlx::Postgres> for LogTopic {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("BYTEA")
    }
}

impl PgHasArrayType for LogTopic {
    fn array_type_info() -> sqlx::postgres::PgTypeInfo {
        <&[u8; 32] as PgHasArrayType>::array_type_info()
    }
}
