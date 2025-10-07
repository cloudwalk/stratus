use super::address::AddressRocksdb;
use super::bytes::BytesRocksdb;
use super::execution_result::ExecutionResultBuilder;
use super::execution_result::ExecutionResultRocksdb;
use super::gas::GasRocksdb;
use super::log::LogRocksdb;
use super::unix_time::UnixTimeRocksdb;
use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::ExecutionChanges;
use crate::eth::primitives::Log;
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;
use crate::ext::OptionExt;

#[derive(Debug, Clone, PartialEq, Eq, bincode::Encode, bincode::Decode, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct ExecutionRocksdb {
    pub block_timestamp: UnixTimeRocksdb,
    pub result: ExecutionResultRocksdb,
    pub output: BytesRocksdb,
    #[deprecated(
        note = "Use logs in transaction mined instead"
    )]
    logs: Vec<LogRocksdb>,
    pub gas: GasRocksdb,
    pub deployed_contract_address: Option<AddressRocksdb>,
}

impl ExecutionRocksdb {
    pub fn new(
        block_timestamp: UnixTimeRocksdb,
        result: ExecutionResultRocksdb,
        output: BytesRocksdb,
        gas: GasRocksdb,
        deployed_contract_address: Option<AddressRocksdb>,
    ) -> Self {
        Self {
            block_timestamp,
            result,
            output,
            logs: Vec::new(),
            gas,
            deployed_contract_address,
        }
    }
}

// impl From<ExecutionRocksdb> for EvmExecution {
//     fn from(item: ExecutionRocksdb) -> Self {
//         let (result, output) = ExecutionResultBuilder((item.result, item.output)).build();
//         Self {
//             block_timestamp: item.block_timestamp.into(),
//             result,
//             output,
//             logs: item.logs.into_iter().map(LogRocksdb::from).collect(),
//             gas_used: item.gas.into(),
//             changes: ExecutionChanges::default(),
//             deployed_contract_address: item.deployed_contract_address.map_into(),
//         }
//     }
// }

impl SerializeDeserializeWithContext for ExecutionRocksdb {}
