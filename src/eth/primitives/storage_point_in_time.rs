use crate::eth::primitives::BlockNumber;

/// EVM storage point-in-time indicator.
#[derive(Debug)]
pub enum StoragerPointInTime {
    /// The current state of the EVM storage.
    Present,

    /// The state of the EVM storage at the given block number.
    Past(BlockNumber),
}
