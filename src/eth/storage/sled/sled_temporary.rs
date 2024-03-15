use async_trait::async_trait;
use sled::Db;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::storage::TemporaryStorage;
use crate::log_and_err;

pub struct SledTemporary {
    db: Db,
}

impl SledTemporary {
    pub fn new() -> anyhow::Result<Self> {
        tracing::info!("starting sled temporary storage");

        let db = match sled::open("data/sled-temp.db") {
            Ok(db) => db,
            Err(e) => return log_and_err!(reason = e, "failed to open sled database"),
        };
        Ok(Self { db })
    }
}

#[async_trait]
impl TemporaryStorage for SledTemporary {
    async fn set_active_block_number(&self, number: BlockNumber) -> anyhow::Result<()> {
        self.db.insert(block_number_key(), number.as_u64().to_be_bytes().to_vec())?;
        Ok(())
    }

    async fn read_active_block_number(&self) -> anyhow::Result<Option<BlockNumber>> {
        match self.db.get(block_number_key()) {
            Ok(Some(number)) => {
                let number = u64::from_be_bytes(number.as_ref().try_into().unwrap());
                Ok(Some(number.into()))
            }
            Ok(None) => Ok(None),
            Err(e) => log_and_err!(reason = e, "failed to read block number from sled"),
        }
    }

    async fn maybe_read_account(&self, address: &Address) -> anyhow::Result<Option<Account>> {
        tracing::debug!(%address, "reading account");

        match self.db.get(account_key(address)) {
            Ok(Some(account)) => {
                let account = serde_json::from_slice(&account).unwrap();
                Ok(Some(account))
            }
            Ok(None) => Ok(None),
            Err(e) => log_and_err!(reason = e, "failed to read account from sled"),
        }
    }

    async fn maybe_read_slot(&self, address: &Address, slot_index: &SlotIndex) -> anyhow::Result<Option<Slot>> {
        tracing::debug!(%address, "reading slot");

        match self.db.get(slot_key(address, slot_index)) {
            Ok(Some(slot)) => {
                let slot = serde_json::from_slice(&slot).unwrap();
                Ok(Some(slot))
            }
            Ok(None) => Ok(None),
            Err(e) => log_and_err!(reason = e, "failed to read slot from sled"),
        }
    }

    async fn save_account_changes(&self, changes: Vec<ExecutionAccountChanges>) -> anyhow::Result<()> {
        let mut batch = sled::Batch::default();
        for change in changes {
            let mut account = self
                .maybe_read_account(&change.address)
                .await?
                .unwrap_or_else(|| Account::new_empty(change.address.clone()));

            // account basic info
            if let Some(nonce) = change.nonce.take() {
                account.nonce = nonce;
            }
            if let Some(balance) = change.balance.take() {
                account.balance = balance;
            }
            if let Some(Some(bytecode)) = change.bytecode.take() {
                account.bytecode = Some(bytecode);
            }
            let account_key = account_key(&change.address).as_bytes().to_vec();
            let account_value = serde_json::to_string(&account).unwrap().as_bytes().to_vec();
            batch.insert(account_key, account_value);

            // slots
            for (_, slot) in change.slots {
                if let Some(slot) = slot.take() {
                    let slot_key = slot_key(&change.address, &slot.index).as_bytes().to_vec();
                    let slot_value = serde_json::to_string(&slot).unwrap().as_bytes().to_vec();
                    batch.insert(slot_key, slot_value);
                }
            }
        }

        // execute batch
        if let Err(e) = self.db.apply_batch(batch) {
            return log_and_err!(reason = e, "failed to apply sled batch");
        }
        Ok(())
    }

    async fn flush_account_changes(&self) -> anyhow::Result<()> {
        if let Err(e) = self.db.flush() {
            return log_and_err!(reason = e, "failed to flush sled data");
        }
        Ok(())
    }

    async fn reset(&self) -> anyhow::Result<()> {
        self.db.clear()?;
        Ok(())
    }
}

fn account_key(address: &Address) -> String {
    format!("address::{}", address)
}

fn slot_key(address: &Address, slot_index: &SlotIndex) -> String {
    format!("slot::{}::{}", address, slot_index)
}

fn block_number_key() -> String {
    "block".to_owned()
}
