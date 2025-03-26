use std::ops::Deref;
use std::sync::Arc;

use revm::primitives::eof::TypesSection;
use revm::primitives::Bytecode;

use super::bytes::BytesRocksdb;
use super::AddressRocksdb;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub enum BytecodeRocksdb {
    LegacyRaw(BytesRocksdb),
    LegacyAnalyzed(LegacyAnalyzedBytecodeRocksdb),
    Eof(EofRocksdb),
    Eip7702(Eip7702BytecodeRocksdb),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub struct LegacyAnalyzedBytecodeRocksdb {
    bytecode: BytesRocksdb,
    original_len: usize,
    jump_table: Vec<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub struct EofHeaderRocksdb {
    pub types_size: u16,
    pub code_sizes: Vec<u16>,
    pub container_sizes: Vec<u16>,
    pub data_size: u16,
    pub sum_code_sizes: usize,
    pub sum_container_sizes: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub struct EofBodyRocksdb {
    pub types_section: Vec<TypesSectionRocksdb>,
    pub code_section: Vec<BytesRocksdb>,
    pub container_section: Vec<BytesRocksdb>,
    pub data_section: BytesRocksdb,
    pub is_data_filled: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub struct TypesSectionRocksdb {
    pub inputs: u8,
    pub outputs: u8,
    pub max_stack_size: u16,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub struct EofRocksdb {
    pub header: EofHeaderRocksdb,
    pub body: EofBodyRocksdb,
    pub raw: BytesRocksdb,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub struct Eip7702BytecodeRocksdb {
    pub delegated_address: AddressRocksdb,
    pub version: u8,
    pub raw: BytesRocksdb,
}

impl From<TypesSection> for TypesSectionRocksdb {
    fn from(value: TypesSection) -> Self {
        Self {
            inputs: value.inputs,
            outputs: value.outputs,
            max_stack_size: value.max_stack_size,
        }
    }
}

impl From<Bytecode> for BytecodeRocksdb {
    fn from(value: Bytecode) -> Self {
        match value {
            Bytecode::LegacyRaw(bytes) => BytecodeRocksdb::LegacyRaw(bytes.to_vec().into()),
            Bytecode::LegacyAnalyzed(analyzed) => BytecodeRocksdb::LegacyAnalyzed(LegacyAnalyzedBytecodeRocksdb {
                bytecode: analyzed.bytecode().clone().into(),
                original_len: analyzed.original_len(),
                jump_table: analyzed.jump_table().0.deref().clone().into_vec(),
            }),
            Bytecode::Eof(eof) => BytecodeRocksdb::Eof(EofRocksdb {
                header: EofHeaderRocksdb {
                    types_size: eof.header.types_size,
                    code_sizes: eof.header.code_sizes.clone(),
                    container_sizes: eof.header.container_sizes.clone(),
                    data_size: eof.header.data_size,
                    sum_code_sizes: eof.header.sum_code_sizes,
                    sum_container_sizes: eof.header.sum_container_sizes,
                },
                body: EofBodyRocksdb {
                    types_section: eof.body.types_section.iter().copied().map(Into::into).collect(),
                    code_section: eof.body.code_section.iter().cloned().map(Into::into).collect(),
                    container_section: eof.body.container_section.iter().cloned().map(Into::into).collect(),
                    data_section: eof.body.data_section.clone().into(),
                    is_data_filled: eof.body.is_data_filled,
                },
                raw: eof.raw.clone().into(),
            }),
            Bytecode::Eip7702(bytecode) => BytecodeRocksdb::Eip7702(Eip7702BytecodeRocksdb {
                delegated_address: AddressRocksdb(bytecode.delegated_address.0 .0),
                version: bytecode.version,
                raw: bytecode.raw.into(),
            }),
        }
    }
}

impl From<BytecodeRocksdb> for Bytecode {
    fn from(value: BytecodeRocksdb) -> Self {
        match value {
            BytecodeRocksdb::LegacyRaw(bytes) => Bytecode::LegacyRaw(bytes.to_vec().into()),
            BytecodeRocksdb::LegacyAnalyzed(analyzed) => Bytecode::LegacyAnalyzed(revm::primitives::LegacyAnalyzedBytecode::new(
                analyzed.bytecode.into(),
                analyzed.original_len,
                revm::primitives::JumpTable::from_slice(&analyzed.jump_table),
            )),
            BytecodeRocksdb::Eof(eof) => {
                let header = revm::primitives::eof::EofHeader {
                    types_size: eof.header.types_size,
                    code_sizes: eof.header.code_sizes,
                    container_sizes: eof.header.container_sizes,
                    data_size: eof.header.data_size,
                    sum_code_sizes: eof.header.sum_code_sizes,
                    sum_container_sizes: eof.header.sum_container_sizes,
                };
                let body = revm::primitives::eof::EofBody {
                    types_section: eof
                        .body
                        .types_section
                        .into_iter()
                        .map(|t| TypesSection {
                            inputs: t.inputs,
                            outputs: t.outputs,
                            max_stack_size: t.max_stack_size,
                        })
                        .collect(),
                    code_section: eof.body.code_section.into_iter().map(Into::into).collect(),
                    container_section: eof.body.container_section.into_iter().map(Into::into).collect(),
                    data_section: eof.body.data_section.into(),
                    is_data_filled: eof.body.is_data_filled,
                };
                Bytecode::Eof(Arc::new(revm::primitives::eof::Eof {
                    header,
                    body,
                    raw: eof.raw.into(),
                }))
            }
            BytecodeRocksdb::Eip7702(bytecode) => Bytecode::Eip7702(revm::primitives::Eip7702Bytecode {
                delegated_address: bytecode.delegated_address.0.into(),
                version: bytecode.version,
                raw: bytecode.raw.into(),
            }),
        }
    }
}
