use revm::bytecode::JumpTable;
use revm::bytecode::LegacyAnalyzedBytecode;
use revm::bytecode::LegacyRawBytecode;
use revm::bytecode::eip7702::Eip7702Bytecode;

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
#[cfg_attr(test, derive(fake::Dummy))]
pub struct Eip7702BytecodeRocksdb {
    pub delegated_address: AddressRocksdb,
    pub version: u8,
    pub raw: BytesRocksdb,
}

impl From<RevmBytecode> for BytecodeRocksdb {
    fn from(value: RevmBytecode) -> Self {
        match value {
            RevmBytecode::LegacyAnalyzed(analyzed) => BytecodeRocksdb::LegacyAnalyzed(LegacyAnalyzedBytecodeRocksdb {
                bytecode: analyzed.bytecode().clone().into(),
                original_len: analyzed.original_len(),
                jump_table: analyzed.jump_table().as_slice().to_vec(),
            }),
            RevmBytecode::Eip7702(bytecode) => BytecodeRocksdb::Eip7702(Eip7702BytecodeRocksdb {
                delegated_address: AddressRocksdb(bytecode.delegated_address.0.0),
                version: bytecode.version,
                raw: bytecode.raw.into(),
            }),
        }
    }
}

impl From<BytecodeRocksdb> for RevmBytecode {
    fn from(value: BytecodeRocksdb) -> Self {
        match value {
            BytecodeRocksdb::LegacyRaw(bytes) => RevmBytecode::LegacyAnalyzed(LegacyRawBytecode(bytes.to_vec().into()).into_analyzed()),
            BytecodeRocksdb::LegacyAnalyzed(analyzed) => RevmBytecode::LegacyAnalyzed(LegacyAnalyzedBytecode::new(
                analyzed.bytecode.into(),
                analyzed.original_len,
                JumpTable::from_slice(&analyzed.jump_table, analyzed.jump_table.len() * 8),
            )),
            BytecodeRocksdb::Eip7702(bytecode) => RevmBytecode::Eip7702(Eip7702Bytecode {
                delegated_address: bytecode.delegated_address.0.into(),
                version: bytecode.version,
                raw: bytecode.raw.into(),
            }),
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
