use derive_more::Debug;

use crate::eth::rpc::BlockFilter;
use crate::eth::types::BlockNumber;
use crate::eth::types::PointInTime;

#[derive(Clone, Copy, serde::Serialize, PartialEq, Default, Eq, Debug)]
#[cfg_attr(test, derive(fake::Dummy))]
pub enum ExecutionKind {
    CallLatest(BlockNumber),
    CallPast(BlockNumber),
    #[default]
    Transaction,
    RPC(PointInTime),
    AccessList,
}

impl ExecutionKind {
    pub fn point_in_time(&self) -> PointInTime {
        self.into()
    }

    pub fn call_from_pit(pit: PointInTime, block_number: BlockNumber) -> Self {
        match pit {
            PointInTime::Latest | PointInTime::Pending => Self::CallLatest(block_number),
            PointInTime::Past(number) => Self::CallPast(number),
        }
    }
}

impl From<&ExecutionKind> for PointInTime {
    fn from(value: &ExecutionKind) -> Self {
        match value {
            ExecutionKind::RPC(pit) => *pit,
            ExecutionKind::Transaction => PointInTime::Pending,
            ExecutionKind::CallPast(number) => PointInTime::Past(*number),
            ExecutionKind::CallLatest(_) | ExecutionKind::AccessList => PointInTime::Latest,
        }
    }
}

impl From<ExecutionKind> for BlockFilter {
    fn from(value: ExecutionKind) -> Self {
        match value {
            ExecutionKind::Transaction | ExecutionKind::RPC(PointInTime::Pending) | ExecutionKind::AccessList => crate::eth::rpc::BlockFilter::Pending,
            ExecutionKind::CallLatest(block_number) | ExecutionKind::CallPast(block_number) => crate::eth::rpc::BlockFilter::Number(block_number),
            ExecutionKind::RPC(pit) => pit.into(),
        }
    }
}
