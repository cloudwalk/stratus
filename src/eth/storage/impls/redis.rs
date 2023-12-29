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

    fn get_current_block(&self) -> Result<BlockNumber, redis::RedisError> {
        let current_block: Result<String, redis::RedisError> = redis::cmd("GET").arg("current_block").query(&mut self.get_connection());
        match current_block {
            Ok(block) => block.into()
            Err(error) => {
                println!("Error {}", error);
                Err(error)
            }
        }
    }

}

impl EthStorage for RedisStorage {

    fn save_block(&self, block: Block) -> Result<(), EthError> {
        let mut con = RedisStorage::get_connection(&self);
        let number: u64 = block.header.number.into();
        let _: () = redis::cmd("SET").arg("current_block").arg(number).execute(&mut con);
        let json = serde_json::to_string(&block).unwrap();
        let key = "BLOCK_".to_string() + &number.to_string();
        let _: () = redis::cmd("SET").arg(key).arg(json).execute(&mut con);
        Ok(())
    }

    fn increment_block_number(&self) -> Result<BlockNumber, EthError> {
        let current_block = self.get_current_block();
        match current_block {
            Ok(block) => {
                let number = block.increment_block_number();
                match number {
                    Ok(res) => {
                        let res = self.set_current_block(res);
                        number
                        // match res {
                        //     Ok(_) => Ok(number)
                        // }
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

    fn read_block(&self, block_number: &BlockSelection) -> Result<Option<Block>, EthError>> {
        let block: Option<Block> = match selection {
            BlockSelection::Latest => redis::cmd("GET").arg("block_1").query(&mut self.get_connection()),
            BlockSelection::Number(number) => redis::cmd("GET").arg("block_"+number).query(&mut self.get_connection()),
            BlockSelection::Hash(hash) => redis::cmd("GET").arg("block_"+hash).query(&mut self.get_connection()),
        }
        
        match block {
            Some(block) => {
                tracing::trace!(?selection, ?block, "block found");
                Ok(Some(block.clone()))
            }
            None => {
                tracing::trace!(?selection, "block not found");
                Ok(None)
            }
        }
    }

    // Implement other methods of EthStorage trait
    // ...
}