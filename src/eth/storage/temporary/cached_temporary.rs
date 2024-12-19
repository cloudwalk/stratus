use std::sync::Arc;

use super::TemporaryStorage;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Hash;
use crate::eth::primitives::PendingBlock;
use crate::eth::primitives::PendingBlockHeader;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StratusError;
use crate::eth::primitives::TransactionExecution;
use crate::eth::storage::cache::CacheConfig;
use crate::eth::storage::StorageCache;

pub struct CachedTemporaryStorage {
    cache: Arc<StorageCache>,
    inmem: Box<dyn TemporaryStorage>,
}

impl CachedTemporaryStorage {
    pub fn new(inmem: Box<dyn TemporaryStorage>, cache_config: &CacheConfig) -> (Self, Arc<StorageCache>) {
        let cache = Arc::new(cache_config.init());
        (
            Self {
                cache: Arc::clone(&cache),
                inmem,
            },
            cache,
        )
    }
    pub fn clear_cache(&self) {
        self.cache.clear();
    }
}

impl TemporaryStorage for CachedTemporaryStorage {
    fn read_pending_block_header(&self) -> PendingBlockHeader {
        self.inmem.read_pending_block_header()
    }

    fn finish_pending_block(&self) -> anyhow::Result<PendingBlock> {
        self.inmem.finish_pending_block()
    }

    fn save_pending_execution(&self, tx: TransactionExecution, check_conflicts: bool) -> Result<(), StratusError> {
        let result = self.inmem.save_pending_execution(tx.clone(), check_conflicts);
        if result.is_ok() {
            self.cache.cache_account_and_slots_from_changes(tx.execution().changes.clone());
        }
        result
    }

    fn read_pending_executions(&self) -> Vec<TransactionExecution> {
        self.inmem.read_pending_executions()
    }

    fn read_pending_execution(&self, hash: Hash) -> anyhow::Result<Option<TransactionExecution>> {
        self.inmem.read_pending_execution(hash)
    }

    fn read_account(&self, address: Address) -> anyhow::Result<Option<Account>> {
        if let Some(account) = self.cache.get_account(address) {
            return Ok(Some(account));
        }
        self.inmem.read_account(address)
    }

    fn read_slot(&self, address: Address, index: SlotIndex) -> anyhow::Result<Option<Slot>> {
        if let Some(slot) = self.cache.get_slot(address, index) {
            return Ok(Some(slot));
        }
        self.inmem.read_slot(address, index)
    }

    fn reset(&self) -> anyhow::Result<()> {
        self.cache.clear();
        self.inmem.reset()
    }
}
