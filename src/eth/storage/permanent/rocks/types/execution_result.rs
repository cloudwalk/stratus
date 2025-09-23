use super::bytes::BytesRocksdb;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ExecutionResult;
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;
#[cfg(test)]
use crate::eth::storage::permanent::rocks::test_utils::FakeEnum;

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    bincode::Encode,
    bincode::Decode,
    fake::Dummy,
    serde::Serialize,
    serde::Deserialize,
    strum::VariantNames,
    stratus_macros::FakeEnum,
)]
#[fake_enum(generate = "crate::utils::test_utils::fake_first")]
// #[derive(Debug, Clone, PartialEq, Eq, bincode::Encode, bincode::Decode, fake::Dummy, serde::Serialize, serde::Deserialize, strum::VariantNames)]
pub enum ExecutionResultRocksdb {
    Success,
    Reverted,
    Halted { reason: String },
}

impl From<ExecutionResult> for ExecutionResultRocksdb {
    fn from(item: ExecutionResult) -> Self {
        match item {
            ExecutionResult::Success => ExecutionResultRocksdb::Success,
            ExecutionResult::Reverted { .. } => ExecutionResultRocksdb::Reverted,
            ExecutionResult::Halted { reason } => ExecutionResultRocksdb::Halted { reason },
        }
    }
}

pub struct ExecutionResultBuilder(pub (ExecutionResultRocksdb, BytesRocksdb));

impl ExecutionResultBuilder {
    pub fn build(self) -> (ExecutionResult, Bytes) {
        self.into()
    }
}

impl From<ExecutionResultBuilder> for (ExecutionResult, Bytes) {
    fn from(item: ExecutionResultBuilder) -> Self {
        let (result, out) = item.0;
        let output: Bytes = out.into();
        match result {
            ExecutionResultRocksdb::Success => (ExecutionResult::Success, output),
            ExecutionResultRocksdb::Reverted => (ExecutionResult::Reverted { reason: (&output).into() }, output),
            ExecutionResultRocksdb::Halted { reason } => (ExecutionResult::Halted { reason }, output),
        }
    }
}

impl SerializeDeserializeWithContext for ExecutionResultRocksdb {}

#[cfg(test)]
mod cf_names {
    use super::*;
    use crate::eth::storage::permanent::rocks::test_utils::ToFileName;
    use crate::impl_to_file_name;

    impl_to_file_name!(ExecutionResultRocksdb, "execution_result");
}
