use std::{collections::HashMap, sync::RwLock};

use anyhow::anyhow;

use crate::eth::{
    primitives::{Account, Address, Block},
    storage::{PermanentStorage, RocksPermanentStorage},
};

use super::inmemory_temporary::InMemoryTemporaryAccount;

struct InmemoryBufferState {
    block: Block,
    accounts: HashMap<Address, InMemoryTemporaryAccount, hash_hasher::HashBuildHasher>,
}

impl From<Block> for InmemoryBufferState {
    fn from(value: Block) -> Self {
        let mut accounts = HashMap::default();
        for change in value.compact_account_changes() {
            if change.is_account_modified() {
                let account = InMemoryTemporaryAccount {
                    info: Account {
                        address: change.address,
                        balance: change.balance.take_modified().unwrap_or_default(),
                        nonce: change.nonce.take_modified().unwrap_or_default(),
                        bytecode: change.bytecode.take_modified().unwrap_or_default(),
                        code_hash: change.code_hash,
                    },
                    slots: change
                        .slots
                        .into_iter()
                        .filter_map(|(index, slot)| slot.take_modified().map(|s| (index, s)))
                        .collect(),
                };
                accounts.insert(change.address, account);
            }
        }
        Self { block: value, accounts }
    }
}

pub struct InmemoryRocksBuffer {
    rocks: RocksPermanentStorage,
    states: RwLock<Vec<InmemoryBufferState>>,
}

impl InmemoryRocksBuffer {
    fn read(&self) -> anyhow::Result<std::sync::RwLockReadGuard<'_, Vec<InmemoryBufferState>>> {
        self.states.read().map_err(|_| anyhow!("failed to acquire lock"))
    }

    fn write(&self) -> anyhow::Result<std::sync::RwLockWriteGuard<'_, Vec<InmemoryBufferState>>> {
        self.states.write().map_err(|_| anyhow!("failed to acquire lock"))
    }
}

impl PermanentStorage for InmemoryRocksBuffer {
    fn read_logs(&self, _filter: &crate::eth::primitives::LogFilter) -> anyhow::Result<Vec<crate::eth::primitives::LogMined>> {
        todo!();
    }

    fn read_slot(
        &self,
        address: &crate::eth::primitives::Address,
        index: &crate::eth::primitives::SlotIndex,
        point_in_time: &crate::eth::storage::StoragePointInTime,
    ) -> anyhow::Result<Option<crate::eth::primitives::Slot>> {
        let states = self.read()?;
        for state in states.iter().rev() {
            if let Some(account) = state.accounts.get(address) {
                if let Some(slot) = account.slots.get(index) {
                    return Ok(Some(*slot));
                }
            }
        }

        self.rocks.read_slot(address, index, point_in_time)
    }

    fn read_block(&self, _block_filter: &crate::eth::primitives::BlockFilter) -> anyhow::Result<Option<crate::eth::primitives::Block>> {
        todo!();
    }

    fn read_account(
        &self,
        address: &crate::eth::primitives::Address,
        point_in_time: &crate::eth::storage::StoragePointInTime,
    ) -> anyhow::Result<Option<crate::eth::primitives::Account>> {
        let states = self.read()?;
        for state in states.iter().rev() {
            if let Some(account) = state.accounts.get(address) {
                return Ok(Some(account.info.clone()));
            }
        }

        self.rocks.read_account(address, point_in_time)
    }

    fn save_block(&self, block: crate::eth::primitives::Block) -> anyhow::Result<()> {
        let mut states = self.write()?;
        states.push(InmemoryBufferState::from(block));

        if states.len() > 2 {
            let oldest_block = states.remove(0);
            self.rocks.save_block(oldest_block.block)?;
        }

        Ok(())
    }

    fn save_accounts(&self, _accounts: Vec<Account>) -> anyhow::Result<()> {
        todo!();
    }

    fn read_transaction(&self, _hash: &crate::eth::primitives::Hash) -> anyhow::Result<Option<crate::eth::primitives::TransactionMined>> {
        todo!();
    }

    fn set_mined_block_number(&self, _number: crate::eth::primitives::BlockNumber) -> anyhow::Result<()> {
        todo!();
    }

    fn read_mined_block_number(&self) -> anyhow::Result<crate::eth::primitives::BlockNumber> {
        let states = self.read()?;
        if let Some(last_state) = states.last() {
            Ok(last_state.block.number())
        } else {
            self.rocks.read_mined_block_number()
        }
    }
}
