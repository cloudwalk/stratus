use crate::eth::types::BlockNumber;
use crate::eth::types::Index;
use crate::eth::types::PointInTime;

#[derive(Clone, Copy, serde::Serialize, PartialEq, Default, Eq)]
#[cfg_attr(test, derive(fake::Dummy))]
pub enum ExecutionKind {
    CallPending(BlockNumber, TxCount),
    CallLatest(BlockNumber),
    CallPast(BlockNumber),
    #[default]
    Transaction,
    RPC(PointInTime),
}

impl ExecutionKind {
    pub fn point_in_time(&self) -> PointInTime {
        self.into()
    }
}

#[derive(Clone, Copy, PartialEq, Debug, serde::Serialize, Eq)]
#[cfg_attr(test, derive(fake::Dummy))]
pub enum TxCount {
    Full,
    Partial(u64),
}

impl TryFrom<TxCount> for Index {
    type Error = anyhow::Error;
    fn try_from(value: TxCount) -> Result<Self, Self::Error> {
        match value {
            TxCount::Partial(idx) => Ok(idx.into()),
            TxCount::Full => anyhow::bail!("full transactions has unknown tx index"),
        }
    }
}

impl From<u64> for TxCount {
    fn from(value: u64) -> Self {
        TxCount::Partial(value)
    }
}

impl Default for TxCount {
    fn default() -> Self {
        TxCount::Partial(0)
    }
}

impl std::ops::AddAssign<u64> for TxCount {
    fn add_assign(&mut self, rhs: u64) {
        match self {
            TxCount::Full => {}                       // If it's Full, keep it Full
            TxCount::Partial(count) => *count += rhs, // If it's Partial, increment the counter
        }
    }
}

impl Ord for TxCount {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (TxCount::Full, TxCount::Full) => std::cmp::Ordering::Equal,
            (TxCount::Full, TxCount::Partial(_)) => std::cmp::Ordering::Greater,
            (TxCount::Partial(_), TxCount::Full) => std::cmp::Ordering::Less,
            (TxCount::Partial(a), TxCount::Partial(b)) => a.cmp(b),
        }
    }
}

impl PartialOrd for TxCount {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl From<&ExecutionKind> for PointInTime {
    fn from(value: &ExecutionKind) -> Self {
        match value {
            ExecutionKind::RPC(pit) => *pit,
            ExecutionKind::Transaction => PointInTime::Pending,
            ExecutionKind::CallPast(number) => PointInTime::Past(*number),
            ExecutionKind::CallLatest(_) => PointInTime::Latest,
            ExecutionKind::CallPending(_, _) => PointInTime::Pending,
        }
    }
}
