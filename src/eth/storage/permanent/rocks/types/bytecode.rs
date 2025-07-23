use revm::bytecode::JumpTable;
use revm::bytecode::LegacyAnalyzedBytecode;
use revm::bytecode::LegacyRawBytecode;
use revm::bytecode::eip7702::Eip7702Bytecode;

use super::AddressRocksdb;
use super::bytes::BytesRocksdb;
use crate::alias::RevmBytecode;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub enum BytecodeRocksdb {
    LegacyRaw(BytesRocksdb),
    LegacyAnalyzed(LegacyAnalyzedBytecodeRocksdb),
    /// Deprecated EOF variant kept to maintain bincode discriminant positions. This prevents existing Eip7702 data from failing deserialization.
    /// TODO: Remove this variant on next database migration when discriminant compatibility is no longer needed.
    EofDeprecated,
    Eip7702(Eip7702BytecodeRocksdb),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub struct LegacyAnalyzedBytecodeRocksdb {
    bytecode: BytesRocksdb,
    original_len: usize,
    jump_table: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
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
            BytecodeRocksdb::EofDeprecated => {
                tracing::error!("encountered deprecated EOF bytecode variant during deserialization, returning empty bytecode");
                RevmBytecode::LegacyAnalyzed(LegacyRawBytecode(vec![].into()).into_analyzed())
            },
            BytecodeRocksdb::Eip7702(bytecode) => RevmBytecode::Eip7702(Eip7702Bytecode {
                delegated_address: bytecode.delegated_address.0.into(),
                version: bytecode.version,
                raw: bytecode.raw.into(),
            }),
        }
    }
}
