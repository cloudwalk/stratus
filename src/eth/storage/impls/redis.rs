extern crate redis;
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


// o que o redis storage precisa fazer
// 1. ler o bloco atual
// 2. incrementar o bloco atual
// 3. traduzir um bloco para um ponto no tempo
// 4. ler uma conta
// 5. ler um slot
// 6. ler um bloco
// 7. ler uma transação
// 8. salvar um bloco
// 9. salvar uma transação

impl RedisStorage {

    fn set_current_block(&self, block_number: BlockNumber) -> () {
        let mut con = self.get_connection();
        let number: u64 = block_number.into();
        let _: () = redis::cmd("SET").arg("current_block").arg(number).execute(&mut con);
    }

    fn get_slot_key(&self, block_number: u64, address: &Address) -> String {
        "SLOT_".to_string() + &block_number.to_string() + &"_".to_string() + &address.to_string()
    }

    fn get_account_key(&self, address: &Address) -> String {
        "ACCOUNT_".to_string() + &address.to_string()
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
        let current_block: Result<u64, redis::RedisError> = redis::cmd("GET").arg("current_block").query(&mut self.get_connection());
        match current_block {
            Ok(block) => {
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

    fn read_account(&self, address: &Address) -> Result<Account, EthError> {
        tracing::debug!(%address, "reading account");

        let key = self.get_account_key(address);
        let account: Result<Option<String>, redis::RedisError> = redis::cmd("GET").arg(key).query(&mut self.get_connection());
        match account {
            Ok(account) => {
                match account {
                    Some(account) => {
                        tracing::trace!(?account, "account found");
                        let account2 = serde_json::from_str(&account).unwrap();
                        Ok(account2)
                    }
                    None => {
                        tracing::trace!("account not found");
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
                tracing::trace!(?error);
                Err(EthError::UnexpectedStorageError)
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
                        tracing::trace!(?value, "slot found");
                        let slot2: HashMap<SlotIndex, Slot> = serde_json::from_str(&value).unwrap();
                        let slot3 = slot2.get(slot).unwrap().clone();
                        Ok(slot3)
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
            BlockSelection::Latest => redis::cmd("GET").arg("block_1").query(&mut self.get_connection()),
            BlockSelection::Number(number) => redis::cmd("GET").arg(self.get_block_key(number)).query(&mut self.get_connection()),
            BlockSelection::Hash(hash) => redis::cmd("GET").arg("BLOCK_".to_owned()+&hash.to_string()).query(&mut self.get_connection()),
        };
        
        match block {
            Ok(block) => {
                tracing::trace!(?selection, ?block, "block found");
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
        let number: u64 = block_number.clone().into();
        let json = serde_json::to_string(&block2.clone()).unwrap();
        let key = self.get_block_key(&block_number);
        let _: () = redis::cmd("SET").arg(key).arg(json).execute(&mut con);
        let _: () = redis::cmd("SET").arg("current_block").arg(number).execute(&mut con);

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
                let _: () = redis::cmd("SET").arg(key).arg(json).execute(&mut con);

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