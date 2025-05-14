use std::ops::Deref;
use std::sync::Arc;

use revm::bytecode::eip7702::Eip7702Bytecode;
use revm::bytecode::eof::CodeInfo;
use revm::bytecode::eof::EofBody;
use revm::bytecode::eof::EofHeader;
use revm::bytecode::Eof;
use revm::bytecode::JumpTable;
use revm::bytecode::LegacyAnalyzedBytecode;
use revm::bytecode::LegacyRawBytecode;

use super::bytes::BytesRocksdb;
use super::AddressRocksdb;
use crate::alias::RevmBytecode;

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
    pub container_sizes: Vec<u32>,
    pub data_size: u16,
    pub sum_code_sizes: usize,
    pub sum_container_sizes: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub struct EofBodyRocksdb {
    pub types_section: Vec<TypesSectionRocksdb>,
    pub code_section: Vec<usize>,
    pub container_section: Vec<BytesRocksdb>,
    pub data_section: BytesRocksdb,
    pub code: BytesRocksdb,
    pub is_data_filled: bool,
    pub code_offset: usize,
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

impl From<CodeInfo> for TypesSectionRocksdb {
    fn from(value: CodeInfo) -> Self {
        Self {
            inputs: value.inputs,
            outputs: value.outputs,
            max_stack_size: value.max_stack_increase,
        }
    }
}

impl From<RevmBytecode> for BytecodeRocksdb {
    fn from(value: RevmBytecode) -> Self {
        match value {
            RevmBytecode::LegacyAnalyzed(analyzed) => BytecodeRocksdb::LegacyAnalyzed(LegacyAnalyzedBytecodeRocksdb {
                bytecode: analyzed.bytecode().clone().into(),
                original_len: analyzed.original_len(),
                jump_table: analyzed.jump_table().0.deref().clone().into_vec(),
            }),
            RevmBytecode::Eof(eof) => BytecodeRocksdb::Eof(EofRocksdb {
                header: EofHeaderRocksdb {
                    types_size: eof.header.types_size,
                    code_sizes: eof.header.code_sizes.clone(),
                    container_sizes: eof.header.container_sizes.clone(),
                    data_size: eof.header.data_size,
                    sum_code_sizes: eof.header.sum_code_sizes,
                    sum_container_sizes: eof.header.sum_container_sizes,
                },
                body: EofBodyRocksdb {
                    types_section: eof.body.code_info.iter().copied().map(Into::into).collect(),
                    code_section: eof.body.code_section.clone(),
                    container_section: eof.body.container_section.iter().cloned().map(Into::into).collect(),
                    data_section: eof.body.data_section.clone().into(),
                    is_data_filled: eof.body.is_data_filled,
                    code: eof.body.code.clone().into(),
                    code_offset: eof.body.code_offset,
                },
                raw: eof.raw.clone().into(),
            }),
            RevmBytecode::Eip7702(bytecode) => BytecodeRocksdb::Eip7702(Eip7702BytecodeRocksdb {
                delegated_address: AddressRocksdb(bytecode.delegated_address.0 .0),
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
                JumpTable::from_slice(&analyzed.jump_table, &analyzed.jump_table.len() * 8),
            )),
            BytecodeRocksdb::Eof(eof) => {
                // This is here to maintain parity with RevmBytecode variants, however currently (2025/05/14) neither Revm
                // nor any of the defined evm specs support this.
                let header = EofHeader {
                    types_size: eof.header.types_size,
                    code_sizes: eof.header.code_sizes,
                    container_sizes: eof.header.container_sizes,
                    data_size: eof.header.data_size,
                    sum_code_sizes: eof.header.sum_code_sizes,
                    sum_container_sizes: eof.header.sum_container_sizes,
                };
                let body = EofBody {
                    code_info: eof
                        .body
                        .types_section
                        .into_iter()
                        .map(|t| CodeInfo {
                            inputs: t.inputs,
                            outputs: t.outputs,
                            max_stack_increase: t.max_stack_size,
                        })
                        .collect(),
                    code_section: eof.body.code_section,
                    code: eof.body.code.into(),
                    container_section: eof.body.container_section.into_iter().map(Into::into).collect(),
                    data_section: eof.body.data_section.into(),
                    is_data_filled: eof.body.is_data_filled,
                    code_offset: eof.body.code_offset,
                };
                RevmBytecode::Eof(Arc::new(Eof {
                    header,
                    body,
                    raw: eof.raw.into(),
                }))
            }
            BytecodeRocksdb::Eip7702(bytecode) => RevmBytecode::Eip7702(Eip7702Bytecode {
                delegated_address: bytecode.delegated_address.0.into(),
                version: bytecode.version,
                raw: bytecode.raw.into(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use revm::bytecode::eof::Eof;
    use std::sync::Arc;

    #[test]
    fn test_eof_conversion() {
        // Create a default Eof value
        let original_eof = Arc::new(Eof::default());

        // Convert Eof to BytecodeRocksdb
        let bytecode_rocksdb = BytecodeRocksdb::from(RevmBytecode::Eof(original_eof.clone()));

        // Convert BytecodeRocksdb back to RevmBytecode
        let converted_back = RevmBytecode::from(bytecode_rocksdb);

        // Check if the original and converted values are equal
        if let RevmBytecode::Eof(eof) = converted_back {
            assert_eq!(*eof, *original_eof);
        } else {
            panic!("Expected RevmBytecode::Eof variant after conversion");
        }
    }
}
