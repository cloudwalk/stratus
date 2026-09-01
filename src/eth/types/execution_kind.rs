use crate::eth::types::BlockNumber;
use crate::eth::types::PointInTime;

#[derive(Clone, Copy, serde::Serialize, PartialEq, Default, Eq)]
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
}

impl From<&ExecutionKind> for PointInTime {
    fn from(value: &ExecutionKind) -> Self {
        match value {
            ExecutionKind::RPC(pit) => *pit,
            ExecutionKind::Transaction | ExecutionKind::AccessList => PointInTime::Pending,
            ExecutionKind::CallPast(number) => PointInTime::Past(*number),
            ExecutionKind::CallLatest(_) => PointInTime::Latest,
        }
    }
}
