use revm::primitives::U256;
use sqlx::FromRow;
use tokio::runtime::Runtime;

use crate::config::Config;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::BlockNumberStorage;
use crate::eth::storage::EthStorage;
use crate::eth::EthError;
use crate::ext::OptionExt;
use crate::infra::postgres::Postgres;

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

impl EthStorage for Postgres {
    fn read_account(&self, address: &Address) -> Result<Account, EthError> {
        tracing::debug!(%address, "reading account");
        let rt = Runtime::new().unwrap();
        let row = rt
            .block_on(async {
                sqlx::query!(
                    r#"
                        SELECT address, nonce, balance, bytecode
                        FROM accounts
                    "#
                )
                .fetch_one(&self.sqlx_pool)
                .await
            })
            .unwrap();

        let account = Account {
            // TODO: use correct Error type for TryInto<Address>
            address: row.address.try_into()?,
            nonce: row.nonce.into(),
            balance: row.balance.into(),
            bytecode: row.bytecode.map_into(),
        };

        Ok(account)
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
