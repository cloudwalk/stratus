use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use async_trait::async_trait;

use super::rocks_state::RocksStorageState;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::storage::InMemoryTemporaryStorage;
use crate::eth::storage::TemporaryStorage;
use crate::log_and_err;

pub struct RocksTemporary {
    temp: InMemoryTemporaryStorage,
    db: RocksStorageState,
    current_block: AtomicU64,
}

impl RocksTemporary {
    pub fn new() -> anyhow::Result<Self> {
        tracing::info!("starting rocks temporary storage");
        let db = RocksStorageState::new();
        let current_block = db.preload_block_number()?;
        Ok(Self {
            temp: InMemoryTemporaryStorage::default(),
            db,
            current_block,
        })
    }
}

#[async_trait]
impl TemporaryStorage for RocksTemporary {
    async fn set_active_block_number(&self, number: BlockNumber) -> anyhow::Result<()> {
        self.current_block.store(number.as_u64(), Ordering::SeqCst);
        self.temp.set_active_block_number(number).await
    }

    async fn read_active_block_number(&self) -> anyhow::Result<Option<BlockNumber>> {
        Ok(Some(self.current_block.load(Ordering::SeqCst).into()))
    }

    async fn maybe_read_account(&self, address: &Address) -> anyhow::Result<Option<Account>> {
        tracing::debug!(%address, "reading account");

        // try temporary data
        let account = self.temp.maybe_read_account(address).await?;
        if let Some(account) = account {
            return Ok(Some(account));
        }

        Ok(self.db.read_account(address, &StoragePointInTime::Present))
    }

    async fn maybe_read_slot(&self, address: &Address, slot_index: &SlotIndex) -> anyhow::Result<Option<Slot>> {
        tracing::debug!(%address, "reading slot");

        // try temporary data
        let slot = self.temp.maybe_read_slot(address, slot_index).await?;
        if let Some(slot) = slot {
            return Ok(Some(slot));
        }

        Ok(self.db.read_slot(address, slot_index, &StoragePointInTime::Present))
    }

    async fn save_account_changes(&self, changes: Vec<ExecutionAccountChanges>) -> anyhow::Result<()> {
        self.temp.save_account_changes(changes).await?;
        Ok(())
    }

    async fn flush(&self) -> anyhow::Result<()> {
        // read before lock
        let Some(number) = self.read_active_block_number().await? else {
            return log_and_err!("no active block number when flushing sled data");
        };

        let mut temp_lock = self.temp.lock_write().await;
        let (accounts, slots): (Vec<Account>, Vec<_>) = temp_lock
            .accounts
            .values()
            .cloned()
            .map(|account| {
                let address = account.info.address.clone();
                let slots = account
                    .slots
                    .values()
                    .cloned()
                    .map(|slot| (address.clone(), slot))
                    .collect::<Vec<(Address, Slot)>>();
                (account.info, slots)
            })
            .unzip();

        self.db.write_accounts(accounts, number);
        self.db.write_slots(slots.into_iter().flatten().collect(), number);

        // reset temporary storage state
        temp_lock.reset();

        Ok(())
    }

    async fn reset(&self) -> anyhow::Result<()> {
        // reset temp
        let mut temp_lock = self.temp.lock_write().await;
        temp_lock.reset();

        // reset sled
        self.db.clear()?;

        Ok(())
    }
}
