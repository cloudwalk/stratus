use crate::eth::rpc::BlockFilter;
use crate::eth::types::BlockNumber;
use crate::eth::types::PointInTime;

/// What kind of work the EVM is executing. Drives the EVM policy (chain-id,
/// nonce and sender validation) and the execution metrics labels.
#[derive(Clone, Copy, serde::Serialize, PartialEq, Eq, Debug, Default)]
#[cfg_attr(test, derive(fake::Dummy))]
pub enum Job {
    #[default]
    Transaction,
    Call,
    /// Plain RPC state read (e.g. `eth_getBalance`), not an execution.
    Rpc,
    AccessList,
}

impl Job {
    pub fn is_transaction(&self) -> bool {
        matches!(self, Job::Transaction)
    }
}

/// Which state the storage reads resolve to.
#[derive(Clone, Copy, serde::Serialize, PartialEq, Eq, Debug, Default)]
#[cfg_attr(test, derive(fake::Dummy))]
pub enum StateView {
    /// State of the pending block being mined: temp reads first, latest state as fallback.
    #[default]
    Pending,

    /// State of the latest mined block. `Some(number)` keeps the requested block
    /// number, so a mid-read staleness can downgrade it to [`StateView::Past`].
    Latest(Option<BlockNumber>),

    /// State at a specific mined block in the past.
    Past(BlockNumber),
}

impl From<PointInTime> for StateView {
    fn from(pit: PointInTime) -> Self {
        match pit {
            PointInTime::Pending => Self::Pending,
            PointInTime::Latest => Self::Latest(None),
            PointInTime::Past(number) => Self::Past(number),
        }
    }
}

impl From<&StateView> for PointInTime {
    fn from(view: &StateView) -> Self {
        match view {
            StateView::Pending => Self::Pending,
            StateView::Latest(_) => Self::Latest,
            StateView::Past(number) => Self::Past(*number),
        }
    }
}

/// What is being executed ([`Job`]) and which state it reads ([`StateView`]).
#[derive(Clone, Copy, serde::Serialize, PartialEq, Eq, Debug, Default)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct ExecutionContext {
    pub job: Job,
    pub at: StateView,
}

impl ExecutionContext {
    /// Local transaction execution against the pending state.
    pub fn transaction() -> Self {
        Self {
            job: Job::Transaction,
            at: StateView::Pending,
        }
    }

    /// Read-only contract call against the given state view.
    pub fn call(at: StateView) -> Self {
        Self { job: Job::Call, at }
    }

    /// Access-list creation against the latest state.
    pub fn access_list() -> Self {
        Self {
            job: Job::AccessList,
            at: StateView::Latest(None),
        }
    }

    /// Contract call at `pit`, with `block_number` as the requested block for pending/latest points.
    pub fn call_from_pit(pit: PointInTime, block_number: BlockNumber) -> Self {
        match pit {
            PointInTime::Latest | PointInTime::Pending => Self::call(StateView::Latest(Some(block_number))),
            PointInTime::Past(number) => Self::call(StateView::Past(number)),
        }
    }

    /// Plain RPC state read at the given point in time.
    #[allow(non_snake_case)]
    pub fn RPC(pit: PointInTime) -> Self {
        Self { job: Job::Rpc, at: pit.into() }
    }

    /// The point in time the state view resolves to.
    pub fn point_in_time(&self) -> PointInTime {
        (&self.at).into()
    }

    /// Prometheus label value, matching the historical `ExecutionKind` AsRefStr strings.
    pub fn metrics_label(&self) -> &'static str {
        match (self.job, self.at) {
            (Job::Transaction, _) => "transaction",
            (Job::AccessList, _) => "access_list",
            (Job::Rpc, StateView::Pending) => "rpc_pending",
            (Job::Rpc, StateView::Latest(_)) => "rpc",
            (Job::Rpc, StateView::Past(_)) => "rpc_past",
            (Job::Call, StateView::Pending) => "call_pending",
            (Job::Call, StateView::Latest(_)) => "call_latest",
            (Job::Call, StateView::Past(_)) => "call_past",
        }
    }
}

impl From<ExecutionContext> for BlockFilter {
    fn from(value: ExecutionContext) -> Self {
        match value {
            ExecutionContext { job: Job::AccessList, .. } => Self::Pending,
            ExecutionContext { at: StateView::Pending, .. } => Self::Pending,
            ExecutionContext {
                at: StateView::Latest(None), ..
            } => Self::Latest,
            ExecutionContext {
                at: StateView::Latest(Some(number)),
                ..
            } => Self::Number(number),
            ExecutionContext {
                at: StateView::Past(number), ..
            } => Self::Number(number),
        }
    }
}
