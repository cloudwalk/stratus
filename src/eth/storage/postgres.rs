use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::traits::BlockNumberStorage;
use crate::eth::storage::traits::EthStorage;
use crate::eth::EthError;

struct PostgresStorage {}

impl EthStorage for PostgresStorage {
    fn read_account(&self, address: &Address) -> Result<Account, EthError> {
        todo!()
    }
    fn read_slot(&self, address: &Address, slot: &SlotIndex) -> Result<Slot, EthError> {
        todo!()
    }
    fn read_block(&self, number: &BlockNumber) -> Result<Option<Block>, EthError> {
        todo!()
    }
    fn read_mined_transaction(&self, hash: &Hash) -> Result<Option<TransactionMined>, EthError> {
        todo!()
    }
    fn save_block(&self, block: Block) -> Result<(), EthError> {
        todo!()
    }
}
