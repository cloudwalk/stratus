use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::current;
use redis::Commands;
extern crate redis;
use crate::infra::redis::RedisStorage;

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
        todo!()
    }

    fn read_slot(&self, address: &Address, slot: &SlotIndex, point_in_time: &StoragerPointInTime) -> Result<Slot, EthError> {
        tracing::debug!(%address, %slot, ?point_in_time, "reading slot");
        todo!()
    }

    fn read_block(&self, selection: &BlockSelection) -> Result<Option<Block>, EthError> {
        tracing::debug!(?selection, "reading block");

        let block: Result<String, redis::RedisError> = match selection {
            BlockSelection::Latest => redis::cmd("GET").arg("block_1").query(&mut self.get_connection()),
            BlockSelection::Number(number) => redis::cmd("GET").arg("BLOCK_".to_owned()+&number.to_string()).query(&mut self.get_connection()),
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
        todo!()
    }

    fn save_block(&self, block: Block) -> Result<(), EthError> {
        let mut con = RedisStorage::get_connection(&self);
        let block2 = block.clone();
        let number: u64 = block.header.number.into();
        let json = serde_json::to_string(&block2.clone()).unwrap();
        let key = "BLOCK_".to_string() + &number.to_string();
        let _: () = redis::cmd("SET").arg(key).arg(json).execute(&mut con);
        let _: () = redis::cmd("SET").arg("current_block").arg(number).execute(&mut con);
        Ok(())
    }

}