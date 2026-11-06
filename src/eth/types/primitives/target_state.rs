use crate::eth::rpc::BlockFilter;
use crate::eth::types::BlockNumber;

/// EVM storage point-in-time indicator.
#[derive(Debug, Clone, Copy, Default, serde::Serialize, PartialEq, Eq, strum::AsRefStr)]
#[strum(serialize_all = "snake_case")]
#[cfg_attr(test, derive(fake::Dummy))]
pub enum TargetState {
    /// State of `Account` or `Slot` at the pending block being mined.
    ///
    /// If the pending state did not change, then it is the same as the `Mined` state.
    Pending,

    /// State of `Account` or `Slot` at the last mined block.
    #[default]
    Latest,

    /// State of `Account` or `Slot` at the last mined block.
    LatestPinned(BlockNumber),

    /// State of `Account` or `Slot` at some specific mined block in the past.
    Past(BlockNumber),
}

impl From<TargetState> for BlockFilter {
    fn from(value: TargetState) -> Self {
        match value {
            TargetState::Pending => BlockFilter::Pending,
            TargetState::LatestPinned(block_number) | TargetState::Past(block_number) => BlockFilter::Number(block_number),
            TargetState::Latest => BlockFilter::Latest,
        }
    }
}
