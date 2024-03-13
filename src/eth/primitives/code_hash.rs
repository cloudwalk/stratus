use std::fmt;

use ethereum_types::H256;
use ethers_core::utils::keccak256;
use revm::primitives::FixedBytes;
use revm::primitives::KECCAK_EMPTY;
use serde::de::SeqAccess;
use serde::de::Visitor;
use sqlx::database::HasValueRef;
use sqlx::error::BoxDynError;

use crate::eth::primitives::Bytes;
use crate::gen_newtype_from;

/// Digest of the bytecode of a contract.
/// In the case of an externally-owned account (EOA), bytecode is null
/// and the code hash is fixed as the keccak256 hash of an empty string
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CodeHash(H256);

impl CodeHash {
    pub fn new(inner: H256) -> Self {
        CodeHash(inner)
    }

    pub fn from_bytecode(maybe_bytecode: Option<Bytes>) -> Self {
        match maybe_bytecode {
            Some(bytecode) => CodeHash(H256::from_slice(&keccak256(bytecode.as_ref()))),
            None => CodeHash::default(),
        }
    }

    pub fn inner(&self) -> H256 {
        self.0
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = CodeHash, other = [u8; 32]);

impl Default for CodeHash {
    fn default() -> Self {
        CodeHash(KECCAK_EMPTY.0.into())
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> other
// -----------------------------------------------------------------------------
impl AsRef<[u8]> for CodeHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl From<FixedBytes<32>> for CodeHash {
    fn from(value: FixedBytes<32>) -> Self {
        CodeHash::new(value.0.into())
    }
}

// -----------------------------------------------------------------------------
// Conversions: sqlx -> Self
// -----------------------------------------------------------------------------
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for CodeHash {
    fn decode(value: <sqlx::Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <[u8; 32] as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        Ok(value.into())
    }
}

impl sqlx::Type<sqlx::Postgres> for CodeHash {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("BYTEA")
    }
}

// // -----------------------------------------------------------------------------
// // Conversions: serde
// // -----------------------------------------------------------------------------
// impl serde::Serialize for CodeHash {
//     fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
//     where
//         S: serde::Serializer,
//     {
//         serializer.serialize_bytes(self.0 .0.as_ref())
//     }
// }

// impl<'de> serde::Deserialize<'de> for CodeHash {
//     fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
//     where
//         D: serde::Deserializer<'de>,
//     {
//         struct CodeHashVisitor;

//         impl<'de> Visitor<'de> for CodeHashVisitor {
//             type Value = CodeHash;

//             fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
//                 formatter.write_str("a byte array representing a H256")
//             }

//             fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
//             where
//                 A: SeqAccess<'de>,
//             {
//                 let mut bytes = Vec::new();
//                 while let Some(byte) = seq.next_element()? {
//                     bytes.push(byte);
//                 }

//                 if bytes.len() != 32 {
//                     return Err(serde::de::Error::invalid_length(bytes.len(), &self));
//                 }

//                 let b256 = H256::from_slice(&bytes);

//                 Ok(CodeHash(b256))
//             }

//             fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
//             where
//                 E: serde::de::Error,
//             {
//                 let b256 = H256::from_slice(v);

//                 Ok(CodeHash(b256))
//             }
//         }

//         deserializer.deserialize_bytes(CodeHashVisitor)
//     }
// }
