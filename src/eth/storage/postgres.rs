use revm::primitives::U256;
use sqlx::{FromRow, PgPool};

use crate::eth::{
    primitives::{Account, Address, Block, BlockNumber, Hash, Slot, SlotIndex, TransactionMined},
    storage::traits::{BlockNumberStorage, EthStorage},
    EthError,
};

type Index = U256;

#[derive(FromRow)]
struct Accounts {
    id: Index,
    account: Account,
}

#[derive(FromRow)]
struct AccountSlots {
    id: Index,
    address: Address,
}

struct Transactions {}

struct Blocks {}

struct Schema {
    accounts: Accounts,
    accounts_slots: AccountSlots,
}

pub struct PostgresStorage {
    pg_pool: PgPool,
    schema: Schema,
}

impl EthStorage for PostgresStorage {
    fn read_account(&self, address: &Address) -> Result<Account, EthError> {
        tracing::debug!(%address, "reading account");
        let account_row = sqlx::query!(
            r#"
                SELECT id, address
                FROM accounts
                ORDER BY id
            "#
        );
        todo!()
    }
    fn read_slot(&self, address: &Address, slot_index: &SlotIndex) -> Result<Slot, EthError> {
        tracing::debug!(%address, %slot_index, "reading slot");
        todo!()
    }
    fn read_block(&self, number: &BlockNumber) -> Result<Option<Block>, EthError> {
        tracing::debug!(%number, "reading block");
        todo!()
    }
    fn read_mined_transaction(&self, hash: &Hash) -> Result<Option<TransactionMined>, EthError> {
        tracing::debug!(%hash, "reading transaction");
        todo!()
    }
    fn save_block(&self, block: Block) -> Result<(), EthError> {
        todo!()
    }
}
