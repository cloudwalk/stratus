#[cfg(test)]
use fake::Dummy;
#[cfg(test)]
use fake::Faker;
use revm::bytecode::BytecodeKind;
use revm::bytecode::JumpTable;
#[cfg(test)]
use revm::bytecode::eip7702::EIP7702_MAGIC_BYTES;
use revm::bytecode::eip7702::EIP7702_VERSION;

use super::AddressRocksdb;
use super::bytes::BytesRocksdb;
use crate::alias::RevmBytecode;
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;
#[cfg(test)]
use crate::eth::storage::permanent::rocks::test_utils::FakeEnum;

#[derive(
    Debug, Clone, PartialEq, Eq, bincode::Encode, bincode::Decode, serde::Serialize, serde::Deserialize, strum::VariantNames, stratus_macros::FakeEnum,
)]
#[cfg_attr(test, derive(fake::Dummy))]
#[fake_enum(generate = "crate::utils::test_utils::fake_first")]
pub enum BytecodeRocksdb {
    LegacyRaw(BytesRocksdb),
    LegacyAnalyzed(LegacyAnalyzedBytecodeRocksdb),
    Eip7702(Eip7702BytecodeRocksdb),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, bincode::Encode, bincode::Decode, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct LegacyAnalyzedBytecodeRocksdb {
    bytecode: BytesRocksdb,
    original_len: usize,
    jump_table: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, bincode::Encode, bincode::Decode, serde::Serialize, serde::Deserialize)]
pub struct Eip7702BytecodeRocksdb {
    pub delegated_address: AddressRocksdb,
    pub version: u8,
    pub raw: BytesRocksdb,
}

/// Manual [`Dummy`] that produces a *valid* EIP-7702 bytecode.
///
/// The derived impl would generate a random `version` and `raw`, which cannot
/// represent a real EIP-7702 entry (only version `0` is valid and `raw` must be
/// the canonical `EF01 00 <address>` 23-byte sequence). Reconstructing from the
/// address keeps the generated value consistent with [`Bytecode::new_eip7702`]
/// and the `From<RevmBytecode>` conversion.
#[cfg(test)]
impl Dummy<Faker> for Eip7702BytecodeRocksdb {
    fn dummy_with_rng<R: rand::Rng + ?Sized>(faker: &Faker, rng: &mut R) -> Self {
        let delegated_address = AddressRocksdb::dummy_with_rng(faker, rng);
        let version = EIP7702_VERSION;
        let mut raw = Vec::with_capacity(23);
        raw.extend_from_slice(EIP7702_MAGIC_BYTES);
        raw.push(version);
        raw.extend_from_slice(&delegated_address.0);
        Self {
            delegated_address,
            version,
            raw: BytesRocksdb(raw),
        }
    }
}

impl From<RevmBytecode> for BytecodeRocksdb {
    fn from(value: RevmBytecode) -> Self {
        match value.kind() {
            BytecodeKind::LegacyAnalyzed => BytecodeRocksdb::LegacyAnalyzed(LegacyAnalyzedBytecodeRocksdb {
                bytecode: value.bytes().into(),
                original_len: value.len(),
                jump_table: value.legacy_jump_table().map(|jt| jt.as_slice().to_vec()).unwrap_or_default(),
            }),
            BytecodeKind::Eip7702 => BytecodeRocksdb::Eip7702(Eip7702BytecodeRocksdb {
                delegated_address: AddressRocksdb(value.eip7702_address().unwrap_or_default().0.0),
                version: EIP7702_VERSION,
                raw: value.bytes().into(),
            }),
        }
    }
}

impl From<BytecodeRocksdb> for RevmBytecode {
    fn from(value: BytecodeRocksdb) -> Self {
        match value {
            BytecodeRocksdb::LegacyRaw(bytes) => RevmBytecode::new_legacy(bytes.into()),
            BytecodeRocksdb::LegacyAnalyzed(analyzed) => unsafe {
                RevmBytecode::new_analyzed(
                    analyzed.bytecode.into(),
                    analyzed.original_len,
                    JumpTable::from_slice(&analyzed.jump_table, analyzed.jump_table.len() * 8),
                )
            },
            BytecodeRocksdb::Eip7702(bytecode) => RevmBytecode::new_eip7702(bytecode.delegated_address.0.into()),
        }
    }
}

impl SerializeDeserializeWithContext for BytecodeRocksdb {}
impl SerializeDeserializeWithContext for LegacyAnalyzedBytecodeRocksdb {}
impl SerializeDeserializeWithContext for Eip7702BytecodeRocksdb {}

#[cfg(test)]
mod cf_names {
    use super::*;
    use crate::eth::storage::permanent::rocks::test_utils::ToFileName;
    use crate::impl_to_file_name;

    impl_to_file_name!(BytecodeRocksdb, "bytecode");
}
