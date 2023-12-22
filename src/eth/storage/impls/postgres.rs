use ethereum_types::H160;
use ethereum_types::U256;

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

        let address = H160::from(address.clone()).0;

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
                    &address
                )
                .fetch_one(&self.connection_pool)
                .await
            })
            .map_err(|e| {
                tracing::error!(reason = ?e, "failed to read address {:?}", address);
                EthError::UnexpectedStorageError
            })?;

        Ok(account)
    }
    fn read_slot(&self, address: &Address, slot_index: &SlotIndex) -> Result<Slot, EthError> {
        tracing::debug!(%address, %slot_index, "reading slot");

        let rt = tokio::runtime::Handle::current();

        let address = H160::from(address.clone()).0;
        let mut buf: [u8; 32] = [1; 32];
        U256::from(slot_index.clone()).to_little_endian(&mut buf);

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
                    &address,
                    &buf
                )
                .fetch_one(&self.connection_pool)
                .await
            })
            .map_err(|e| {
                tracing::error!(reason = ?e, "failed to read slot index {:?} from address {:?}", slot_index, address);
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
        tracing::debug!("saving block {:?}", _block);
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
