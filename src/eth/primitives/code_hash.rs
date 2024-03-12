use revm::primitives::B256;
use revm::primitives::KECCAK_EMPTY;
use sqlx::database::HasValueRef;
use sqlx::error::BoxDynError;

use crate::eth::primitives::Bytes;
use crate::gen_newtype_from;

/// Representation of the bytecode of a contract account.
/// In the case of an externally-owned account (EOA), bytecode is null
/// and the code hash is fixed as the keccak256 hash of an empty string
#[derive(Debug, Clone)]
pub struct CodeHash(B256);

impl CodeHash {
    pub fn from_bytecode(maybe_bytecode: Option<Bytes>) -> Self {
        match maybe_bytecode {
            Some(bytecode) => CodeHash(B256::from_slice(bytecode.as_ref())),
            None => CodeHash::default(),
        }
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = CodeHash, other = [u8; 32]);

impl Default for CodeHash {
    fn default() -> Self {
        CodeHash(KECCAK_EMPTY)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> other
// -----------------------------------------------------------------------------
impl AsRef<[u8]> for CodeHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
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
