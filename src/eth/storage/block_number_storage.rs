use crate::eth::primitives::BlockNumber;
use crate::eth::EthError;

/// Block number storage operations.
pub trait BlockNumberStorage: Send + Sync + 'static {
    // Retrieves the last mined block number.
    fn current_block_number(&self) -> Result<BlockNumber, EthError>;

    /// Atomically increments the block number, returning the new value.
    fn increment_block_number(&self) -> Result<BlockNumber, EthError>;
}
