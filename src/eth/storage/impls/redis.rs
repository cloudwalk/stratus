extern crate redis;
use crate::eth::primitives::TransactionExecutionValueChange;
pub use crate::infra::redis::RedisStorage;
use std::collections::HashMap;

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

impl RedisStorage {

    fn set_current_block(&self, block_number: BlockNumber) -> () {
        let mut con = self.get_connection();
        let number: u64 = block_number.into();
        let _: () = redis::cmd("SET").arg("CURRENT_BLOCK").arg(number).execute(&mut con);
    }

    fn get_slot_key(&self, block_number: u64, address: &Address) -> String {
        "SLOT_".to_string() + &block_number.to_string() + &"_".to_string() + &address.to_string()
    }

    fn get_account_key(&self, address: &Address) -> String {
        "ACCOUNT_".to_string() + &address.to_string()
    }

    fn get_account_block_key(&self, address: &Address, block_number: &BlockNumber) -> String {
        "ACCOUNT_".to_string() + &address.to_string() + &"_BLOCK_".to_string() + &block_number.to_string()
    }

    fn get_block_key(&self, block_number: &BlockNumber) -> String {
        let number: u64 = block_number.clone().into();
        "BLOCK_".to_string() + &number.to_string()
    }

    fn get_transaction_key(&self, hash: &Hash) -> String {
        "TRANSACTION_".to_string() + &hash.to_string()
    }

}

impl EthStorage for RedisStorage {

    fn read_current_block_number(&self) -> Result<BlockNumber, EthError> {
        tracing::debug!("read_current_block_number");
        let current_block: Result<u64, redis::RedisError> = redis::cmd("GET").arg("CURRENT_BLOCK").query(&mut self.get_connection());
        match current_block {
            Ok(block) => {
                tracing::debug!("read_current_block_number={:?}",block);
                let block_number: BlockNumber = block.into();
                Ok(block_number)
            }
            Err(error) => {
                println!("Error {}", error);
                Err(EthError::UnexpectedStorageError)
            }
        }
    }

    fn increment_block_number(&self) -> Result<BlockNumber, EthError> {
        tracing::debug!("increment_block_number");
        let current_block = self.read_current_block_number();
        match current_block {
            Ok(block) => {
                let number = block.increment_block_number();
                match number {
                    Ok(res) => {
                        tracing::debug!("{:?}",res);
                        self.set_current_block(res.clone());
                        Ok(res)
                    },
                    Err(error) => Err(error)
                }
            },
            Err(error) => {
                println!("Error {}", error);
                Err(EthError::UnexpectedStorageError)
            }
        }
    }

    fn read_account(&self, address: &Address, point_in_time: &StoragerPointInTime) -> Result<Account, EthError> {
        tracing::debug!(%address, "reading account");

        let key = self.get_account_key(address);
        match point_in_time {
            StoragerPointInTime::Present => {
                let account: Result<Option<String>, redis::RedisError> = redis::cmd("GET").arg(key).query(&mut self.get_connection());
                match account {
                    Ok(account) => {
                        match account {
                            Some(account) => {
                                let account2 = serde_json::from_str(&account).unwrap();
                                tracing::debug!("account found {:?}",account2);
                                Ok(account2)
                            }
                            None => {
                                tracing::debug!("account not found");
                                let zero: u64 = 0;
                                Ok(Account {
                                    address: address.clone(),
                                    nonce: zero.into(),
                                    balance: zero.into(),
                                    bytecode: None
                                })
                            }
                        }
                    }
                    Err(error) => {
                        tracing::debug!(?error);
                        let account = Account {
                            address: address.clone(),
                            nonce: 0.into(),
                            balance: 0.into(),
                            bytecode: None
                        };
                        tracing::debug!(?account, "didn't found on redis");
                        Ok(account)
                        // Err(EthError::UnexpectedStorageError)
                    }
                }
            }
            StoragerPointInTime::Past(current_block) => {
                let key = self.get_account_block_key(address, current_block);
                let account: Result<Option<String>, redis::RedisError> = redis::cmd("GET").arg(key).query(&mut self.get_connection());
                match account {
                    Ok(account) => {
                        match account {
                            Some(account) => {
                                let account2 = serde_json::from_str(&account).unwrap();
                                tracing::debug!(?account2, "account found");
                                Ok(account2)
                            }
                            None => {
                                tracing::debug!("account not found");
                                let zero: u64 = 0;
                                Ok(Account {
                                    address: address.clone(),
                                    nonce: zero.into(),
                                    balance: zero.into(),
                                    bytecode: None
                                })
                            }
                        }
                    }
                    Err(error) => {
                        tracing::debug!(?error);
                        let account = Account {
                            address: address.clone(),
                            nonce: 0.into(),
                            balance: 0.into(),
                            bytecode: None
                        };
                        tracing::debug!(?account, "didn't found on redis");
                        Ok(account)
                    }
                }
            }
        }
    }

    fn read_slot(&self, address: &Address, slot: &SlotIndex, point_in_time: &StoragerPointInTime) -> Result<Slot, EthError> {
        tracing::debug!(%address, %slot, ?point_in_time, "reading slot");

        let block = match point_in_time {
            StoragerPointInTime::Present => { self.read_current_block_number()? }
            StoragerPointInTime::Past(current_block) => { current_block.clone() }
        };

        let key = self.get_slot_key(block.into(), address);
        let value: Result<Option<String>, redis::RedisError> = redis::cmd("GET").arg(key).query(&mut self.get_connection());
        match value {
            Ok(value) => {
                match value {
                    Some(value) => {
                        tracing::debug!(?value, "slot found");
                        let slot2: HashMap<SlotIndex, TransactionExecutionValueChange<_>> = serde_json::from_str(&value).unwrap();
                        tracing::debug!(?slot2, "slot found");
                        let changes = slot2.get(&slot);//.unwrap().clone();
                        match changes {
                            Some(changes2) => {
                                tracing::trace!(?changes2, "slot found");
                                let some = changes2.clone().take_if_modified();
                                match some {
                                    Some(some) => Ok(some),
                                    None => {
                                        let some = changes2.clone().take_if_original();
                                        match some {
                                            Some(some) => Ok(some),
                                            None => Ok(Slot::default())
                                        }
                                    }
                                }
                            }
                            None => Ok(Slot::default())
                        }
                            None => Ok(Slot::default())
                    }
                    None => {
                        tracing::trace!("slot not found");
                        Ok(Slot::default())
                    }
                }
            }
            Err(error) => {
                tracing::trace!(?error);
                Err(EthError::UnexpectedStorageError)
            }
        }
    }

    fn read_block(&self, selection: &BlockSelection) -> Result<Option<Block>, EthError> {
        tracing::debug!(?selection, "reading block");

        let block: Result<String, redis::RedisError> = match selection {
            BlockSelection::Latest => {
                let number = self.read_current_block_number()?;
                redis::cmd("GET").arg("BLOCK_".to_string() + &number.to_string()).query(&mut self.get_connection())
            },
            BlockSelection::Number(number) => redis::cmd("GET").arg(self.get_block_key(number)).query(&mut self.get_connection()),
            BlockSelection::Hash(hash) => redis::cmd("GET").arg("BLOCK_".to_owned()+&hash.to_string()).query(&mut self.get_connection()),
        };
        
        match block {
            Ok(block) => {
                tracing::debug!("block found {:?}", block);
                let block2 = serde_json::from_str(&block).unwrap();
                Ok(block2)
            }
            Err(error) => {
                tracing::trace!(?error);
                Err(EthError::UnexpectedStorageError)
            }
        }
    }

    fn read_mined_transaction(&self, hash: &Hash) -> Result<Option<TransactionMined>, EthError> {
        tracing::debug!(%hash, "reading transaction");
        // read transaction
        let key = self.get_transaction_key(hash);
        let transaction: Result<Option<String>, redis::RedisError> = redis::cmd("GET").arg(key).query(&mut self.get_connection());
        match transaction {
            Ok(transaction) => {
                match transaction {
                    Some(transaction) => {
                        tracing::trace!(?transaction, "transaction found");
                        let transaction2 = serde_json::from_str(&transaction).unwrap();
                        Ok(Some(transaction2))
                    }
                    None => {
                        tracing::trace!("transaction not found");
                        Ok(None)
                    }
                }
            }
            Err(error) => {
                tracing::trace!(?error);
                Err(EthError::UnexpectedStorageError)
            }
        }
    }

    fn save_block(&self, block: Block) -> Result<(), EthError> {
        tracing::debug!(number = %block.header.number, "saving block");
        let mut con = RedisStorage::get_connection(&self);

        // save block
        let block2 = block.clone();
        let block_number = block.header.number;
        tracing::debug!(number = %block_number, "saving block {:?}", block_number);
        let number: u64 = block_number.clone().into();
        let json = serde_json::to_string(&block2.clone()).unwrap();
        let key = self.get_block_key(&block_number);
        let _: () = redis::cmd("SET").arg(key).arg(json).execute(&mut con);
        let _: () = redis::cmd("SET").arg("CURRENT_BLOCK").arg(number).execute(&mut con);

        // save account data (nonce, balance, bytecode)
        for transaction in block.transactions {
            tracing::debug!(hash = %transaction.input.hash, "saving transaction");

            // save transaction
            let key = self.get_transaction_key(&transaction.input.hash);
            let json = serde_json::to_string(&transaction.clone()).unwrap();
            let _: () = redis::cmd("SET").arg(key).arg(json).execute(&mut con);

            // save execution changes
            let is_success = transaction.is_success();
            for mut changes in transaction.execution.changes {
                let key = self.get_account_key(&changes.address);
                let account: Result<Option<String>, redis::RedisError> = redis::cmd("GET").arg(key.clone()).query(&mut con);
                let mut account: Account = match account {
                    Ok(data) => { 
                        match data {
                            Some(data) => serde_json::from_str(&data).unwrap(),
                            None => {
                                let zero: u64 = 0;
                                Account { 
                                    address: changes.address.clone(),
                                    nonce: zero.into(),
                                    balance: zero.into(),
                                    bytecode: None
                                }
                            }
                        }
                    }
                    Err(error) => { 
                        tracing::trace!(?error);
                        panic!("Error {}", error);
                    }
                };

                // nonce
                if let Some(nonce) = changes.nonce.take_if_modified() {
                    account.nonce = nonce;
                }

                // balance
                if let Some(balance) = changes.balance.take_if_modified() {
                    account.balance = balance;
                }

                // bytecode
                if is_success {
                    if let Some(Some(bytecode)) = changes.bytecode.take_if_modified() {
                        tracing::trace!(bytecode_len = %bytecode.len(), "saving bytecode");
                        account.bytecode = Some(bytecode);
                    }
                }

                let json = serde_json::to_string(&account.clone()).unwrap();
                let _: () = redis::cmd("SET").arg(key).arg(&json).execute(&mut con);

                let key_account_block = self.get_account_block_key(&changes.address, &block_number);
                let _: () = redis::cmd("SET").arg(key_account_block).arg(&json).execute(&mut con);

                // store slots of a contract
                if is_success {
                    let key = self.get_slot_key(number, &changes.address);
                    let slots = changes.slots;
                    let json = serde_json::to_string(&slots).unwrap();
                    let _ = redis::cmd("SET").arg(key).arg(json).execute(&mut con);
                }
            }
        }
        Ok(())
    }
}