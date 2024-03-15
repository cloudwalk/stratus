use async_trait::async_trait;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::storage::InMemoryTemporaryStorage;
use crate::eth::storage::TemporaryStorage;
use crate::log_and_err;

pub struct SledTemporary {
    // keep transactional data in here until flush is called
    // sled supports transactions, but I still have to learn how to use it because it accepts a closure instead of returning an object
    temp: InMemoryTemporaryStorage,
    db: sled::Db,
}

impl SledTemporary {
    pub fn new() -> anyhow::Result<Self> {
        tracing::info!("starting sled temporary storage");

        let options = sled::Config::new().mode(sled::Mode::HighThroughput).path("data/sled-temp.db");
        let db = match options.open() {
            Ok(db) => db,
            Err(e) => return log_and_err!(reason = e, "failed to open sled database"),
        };
        Ok(Self {
            temp: InMemoryTemporaryStorage::default(),
            db,
        })
    }
}

#[async_trait]
impl TemporaryStorage for SledTemporary {
    async fn set_active_block_number(&self, number: BlockNumber) -> anyhow::Result<()> {
        self.temp.set_active_block_number(number).await
    }

    async fn read_active_block_number(&self) -> anyhow::Result<Option<BlockNumber>> {
        // try temporary data
        let number = self.temp.read_active_block_number().await?;
        if let Some(number) = number {
            return Ok(Some(number));
        }

        // try durable data
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

        // try temporary data
        let account = self.temp.maybe_read_account(address).await?;
        if let Some(account) = account {
            return Ok(Some(account));
        }

        // try durable data
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

        // try temporary data
        let slot = self.temp.maybe_read_slot(address, slot_index).await?;
        if let Some(slot) = slot {
            return Ok(Some(slot));
        }

        // try durable data
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
        self.temp.save_account_changes(changes).await?;
        Ok(())
    }

    async fn flush_account_changes(&self) -> anyhow::Result<()> {
        let mut temp = self.temp.lock_write().await;

        let Some(number) = self.read_active_block_number().await? else {
            return log_and_err!("no active block number when flushing sled data");
        };

        let tx_result = self.db.transaction::<_, (), anyhow::Error>(|tx| {
            for account in temp.accounts.values() {
                // write account
                let account_key = account_key_vec(&account.info.address);
                let account_value = serde_json::to_string(&account.info).unwrap().as_bytes().to_vec();
                tx.insert(account_key, account_value)?;

                // write slots
                for slot in account.slots.values() {
                    let slot_key = slot_key_vec(&account.info.address, &slot.index);
                    let slot_value = serde_json::to_string(&slot).unwrap().as_bytes().to_vec();
                    tx.insert(slot_key, slot_value)?;
                }
            }

            // sets the next active block number
            tx.insert(block_number_key_vec(), serialize_number(number.next()))?;

            Ok(())
        });
        if let Err(e) = tx_result {
            tracing::error!(reason = ?e, "failed to transact sled data");
            return match e {
                sled::transaction::TransactionError::Abort(e) => Err(e),
                sled::transaction::TransactionError::Storage(e) => Err(e.into()),
            };
        }

        // flush
        if let Err(e) = self.db.flush() {
            return log_and_err!(reason = e, "failed to flush sled data");
        }

        // reset temporary storage state
        temp.reset();

        Ok(())
    }

    async fn reset(&self) -> anyhow::Result<()> {
        self.db.clear()?;
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Keys
// -----------------------------------------------------------------------------

fn account_key_vec(address: &Address) -> Vec<u8> {
    account_key(address).into_bytes().to_vec()
}

fn account_key(address: &Address) -> String {
    format!("address::{}", address)
}

fn slot_key_vec(address: &Address, slot_index: &SlotIndex) -> Vec<u8> {
    slot_key(address, slot_index).into_bytes().to_vec()
}

fn slot_key(address: &Address, slot_index: &SlotIndex) -> String {
    format!("slot::{}::{}", address, slot_index)
}

fn block_number_key_vec() -> Vec<u8> {
    block_number_key().into_bytes().to_vec()
}

fn block_number_key() -> String {
    "block".to_owned()
}

fn serialize_number(number: BlockNumber) -> Vec<u8> {
    number.as_u64().to_be_bytes().to_vec()
}
