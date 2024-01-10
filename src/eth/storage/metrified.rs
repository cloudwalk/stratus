use std::time::Instant;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StoragerPointInTime;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::EthStorage;
use crate::eth::EthError;
use crate::infra::metrics;

pub struct MetrifiedStorage<T: EthStorage> {
    inner: T,
}

impl<T: EthStorage> MetrifiedStorage<T> {
    pub fn new(inner: T) -> Self {
        Self { inner }
    }
}

impl<T: EthStorage> EthStorage for MetrifiedStorage<T> {
    fn read_current_block_number(&self) -> Result<BlockNumber, EthError> {
        self.inner.read_current_block_number()
    }

    fn increment_block_number(&self) -> Result<BlockNumber, EthError> {
        self.inner.increment_block_number()
    }

    fn read_account(&self, address: &Address, point_in_time: &StoragerPointInTime) -> Result<Account, EthError> {
        let start = Instant::now();
        let result = self.inner.read_account(address, point_in_time);
        metrics::inc_storage_accounts_read(start.elapsed(), result.is_ok());
        result
    }

    fn read_slot(&self, address: &Address, slot: &SlotIndex, point_in_time: &StoragerPointInTime) -> Result<Slot, EthError> {
        let start = Instant::now();
        let result = self.inner.read_slot(address, slot, point_in_time);
        metrics::inc_storage_slots_read(start.elapsed(), result.is_ok());
        result
    }

    fn read_block(&self, block_selection: &BlockSelection) -> Result<Option<Block>, EthError> {
        self.inner.read_block(block_selection)
    }

    fn read_mined_transaction(&self, hash: &Hash) -> Result<Option<TransactionMined>, EthError> {
        self.inner.read_mined_transaction(hash)
    }

    fn save_block(&self, block: Block) -> Result<(), EthError> {
        self.inner.save_block(block)
    }
}
