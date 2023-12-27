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

impl EthStorage for Postgres {
    fn read_account(&self, address: &Address) -> Result<Account, EthError> {
        tracing::debug!(%address, "reading account");

        let rt = tokio::runtime::Handle::current();

        let account = rt
            .block_on(async {
                sqlx::query_as!(
                    Account,
                    r#"
                        SELECT 
                            address as "address: _", 
                            nonce as "nonce: _", 
                            balance as "balance: _",
                            bytecode as "bytecode: _"
                        FROM accounts
                        WHERE address = $1
                    "#,
                    address.as_ref()
                )
                .fetch_one(&self.connection_pool)
                .await
            })
            .map_err(|e| {
                tracing::error!(reason = ?e, address = ?address, "Failed to read address");
                EthError::UnexpectedStorageError
            })?;

        Ok(account)
    }
    fn read_slot(&self, address: &Address, slot_index: &SlotIndex) -> Result<Slot, EthError> {
        tracing::debug!(%address, %slot_index, "reading slot");

        let rt = tokio::runtime::Handle::current();

        // TODO: improve this
        let slot_index: [u8; 32] = slot_index.clone().into();

        let slot = rt
            .block_on(async {
                sqlx::query_as!(
                    Slot,
                    r#"
                        SELECT 
                            idx as "index: _", 
                            value as "value: _"
                        FROM account_slots
                        WHERE account_address = $1 AND idx = $2
                    "#,
                    address.as_ref(),
                    slot_index.as_ref()
                )
                .fetch_one(&self.connection_pool)
                .await
            })
            .map_err(|e| {
                tracing::error!(reason = ?e, index = ?slot_index, address = ?address, "Failed to read slot index");
                EthError::UnexpectedStorageError
            })?;

        Ok(slot)
    }
    fn read_block(&self, number: &BlockNumber) -> Result<Option<Block>, EthError> {
        tracing::debug!(%number, "reading block");
        todo!()
    }
    fn read_mined_transaction(&self, hash: &Hash) -> Result<Option<TransactionMined>, EthError> {
        tracing::debug!(%hash, "reading transaction");
        todo!()
    }
    fn save_block(&self, _block: Block) -> Result<(), EthError> {
        tracing::debug!(block = ?_block, "saving block");
        todo!()
    }
}

impl BlockNumberStorage for Postgres {
    fn current_block_number(&self) -> Result<BlockNumber, EthError> {
        tracing::debug!("reading current block number");

        let rt = tokio::runtime::Handle::current();

        let currval: i64 = rt
            .block_on(async {
                sqlx::query_scalar!(
                    r#"
                        SELECT CURRVAL('block_number_seq') as "n!: _"
                    "#
                )
                .fetch_one(&self.connection_pool)
                .await
            })
            .map_err(|e| {
                tracing::error!(reason = ?e, "failed to retrieve sequence 'block_number_seq' current value");
                EthError::UnexpectedStorageError
            })?;

        let block_number = BlockNumber::from(currval);

        Ok(block_number)
    }

    fn increment_block_number(&self) -> Result<BlockNumber, EthError> {
        tracing::debug!("incrementing block number");

        let rt = tokio::runtime::Handle::current();

        let nextval: i64 = rt
            .block_on(async {
                sqlx::query_scalar!(
                    r#"
                        SELECT NEXTVAL('block_number_seq') as "n!: _"
                    "#
                )
                .fetch_one(&self.connection_pool)
                .await
            })
            .map_err(|e| {
                tracing::error!(reason = ?e, "failed to retrieve sequence 'block_number_seq' next value");
                EthError::UnexpectedStorageError
            })?;

        let block_number = BlockNumber::from(nextval);

        Ok(block_number)
    }
}
