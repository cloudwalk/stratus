use revm::primitives::U256;
use sqlx::FromRow;
use sqlx::PgPool;
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

pub struct PostgresStorage {
    pg_pool: PgPool,
    // schema: Schema,
}

impl PostgresStorage {
    async fn new(cfg: &Config) -> Self {
        let pg_pool = Postgres::new(&cfg.database_url).await.unwrap().sqlx_pool;
        Self { pg_pool }
    }
}

impl EthStorage for PostgresStorage {
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
                .fetch_one(&self.pg_pool)
                .await
            })
            .unwrap();

        // let account = Account {
        //     address: row.address.into(),
        //     nonce: row.nonce.into(),
        //     balance: row.balance.into(),
        //     bytecode: row.bytecode,
        // };
        let account = Account::default();

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
