use std::fmt::Debug;
use std::fmt::Display;
use std::ops::Deref;

use ethers_core::types::Bytes as EthersBytes;
use revm::primitives::Bytecode as RevmBytecode;
use revm::primitives::Bytes as RevmBytes;
use revm::primitives::Output as RevmOutput;
use sqlx::database::HasValueRef;
use sqlx::error::BoxDynError;

use crate::gen_newtype_from;

#[derive(Clone, Default, Eq, PartialEq, fake::Dummy)]
pub struct Bytes(Vec<u8>);

impl Display for Bytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.len() <= 256 {
            write!(f, "{}", const_hex::encode_prefixed(&self.0))
        } else {
            write!(f, "too long")
        }
    }
}

impl Debug for Bytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Bytes").field(&self.to_string()).finish()
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
gen_newtype_from!(self = Bytes, other = Vec<u8>, &[u8]);

impl From<EthersBytes> for Bytes {
    fn from(value: EthersBytes) -> Self {
        Self(value.into_iter().collect())
    }
}

impl From<RevmBytecode> for Bytes {
    fn from(value: RevmBytecode) -> Self {
        Self(value.bytecode.0.into_iter().collect())
    }
}

impl From<RevmBytes> for Bytes {
    fn from(value: RevmBytes) -> Self {
        Self(value.0.into_iter().collect())
    }
}

impl From<&RevmBytes> for Bytes {
    fn from(value: &RevmBytes) -> Self {
        Self(value.to_vec())
    }
}

impl From<RevmOutput> for Bytes {
    fn from(value: RevmOutput) -> Self {
        match value {
            RevmOutput::Call(bytes) => bytes.into(),
            RevmOutput::Create(bytes, _) => bytes.into(),
        }
    }
}

// -----------------------------------------------------------------------------
// Conversions: sqlx -> Self
// -----------------------------------------------------------------------------
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for Bytes {
    fn decode(value: <sqlx::Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <Vec<u8> as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        Ok(value.into())
    }
}

impl sqlx::Type<sqlx::Postgres> for Bytes {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("BYTEA")
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

impl From<Bytes> for EthersBytes {
    fn from(value: Bytes) -> Self {
        value.0.into()
    }
}

impl From<Bytes> for RevmBytes {
    fn from(value: Bytes) -> Self {
        value.0.into()
    }
}

impl From<Bytes> for RevmBytecode {
    fn from(value: Bytes) -> Self {
        RevmBytecode::new_raw(value.0.into())
    }
}
