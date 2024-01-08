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
use crate::infra::postgres::Postgres;

impl EthStorage for Postgres {
    fn read_account(&self, address: &Address, point_in_time: &StoragerPointInTime) -> Result<Account, EthError> {
        tracing::debug!(%address, "reading account");

        let rt = tokio::runtime::Handle::current();

        // TODO: use HistoricalValue
        let block = match point_in_time {
            StoragerPointInTime::Present => self.read_current_block_number()?,
            StoragerPointInTime::Past(number) => *number,
        };

        let block_number = i64::try_from(block).map_err(|_| EthError::StorageConvertError {
            from: "BlockNumber".to_string(),
            into: "i64".to_string(),
        })?;

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
                        WHERE address = $1 AND block_number = $2
                    "#,
                    address.as_ref(),
                    block_number,
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
    fn read_slot(&self, address: &Address, slot_index: &SlotIndex, point_in_time: &StoragerPointInTime) -> Result<Slot, EthError> {
        tracing::debug!(%address, %slot_index, "reading slot");

        let rt = tokio::runtime::Handle::current();

        // TODO: use HistoricalValue
        let block = match point_in_time {
            StoragerPointInTime::Present => self.read_current_block_number()?,
            StoragerPointInTime::Past(number) => *number,
        };

        let block_number = i64::try_from(block).map_err(|_| EthError::StorageConvertError {
            from: "BlockNumber".to_string(),
            into: "i64".to_string(),
        })?;

        // TODO: improve this conversion
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
                        WHERE account_address = $1 AND idx = $2 AND block_number = $3
                    "#,
                    address.as_ref(),
                    slot_index.as_ref(),
                    block_number,
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
    fn read_block(&self, block: &BlockSelection) -> Result<Option<Block>, EthError> {
        tracing::debug!(block = ?block, "reading block");
        todo!()
    }
    fn read_mined_transaction(&self, hash: &Hash) -> Result<Option<TransactionMined>, EthError> {
        tracing::debug!(%hash, "reading transaction");
        todo!()
    }
    fn save_block(&self, block: Block) -> Result<(), EthError> {
        tracing::debug!(block = ?block, "saving block");
        todo!()
    }
    fn read_current_block_number(&self) -> Result<BlockNumber, EthError> {
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
