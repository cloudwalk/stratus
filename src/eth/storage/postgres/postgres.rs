use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockHeader;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::postgres::types::PostgresLogs;
use crate::eth::storage::postgres::types::PostgresTopic;
use crate::eth::storage::postgres::types::PostgresTransaction;
use crate::eth::storage::EthStorage;
use crate::eth::EthError;
use crate::infra::postgres::Postgres;

impl EthStorage for Postgres {
    fn read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> Result<Account, EthError> {
        tracing::debug!(%address, "reading account");

        let rt = tokio::runtime::Handle::current();

        // TODO: use HistoricalValue
        let block = match point_in_time {
            StoragePointInTime::Present => self.read_current_block_number()?,
            StoragePointInTime::Past(number) => *number,
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
    fn read_slot(&self, address: &Address, slot_index: &SlotIndex, point_in_time: &StoragePointInTime) -> Result<Slot, EthError> {
        tracing::debug!(%address, %slot_index, "reading slot");

        let rt = tokio::runtime::Handle::current();

        // TODO: use HistoricalValue
        let block = match point_in_time {
            StoragePointInTime::Present => self.read_current_block_number()?,
            StoragePointInTime::Past(number) => *number,
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

        let rt = tokio::runtime::Handle::current();

        match block {
            BlockSelection::Latest => {
                let current = self.read_current_block_number()?;

                let block_number = i64::try_from(current).map_err(|_| EthError::StorageConvertError {
                    from: "BlockNumber".to_string(),
                    into: "i64".to_string(),
                })?;

                rt.block_on(async {
                    let _header = sqlx::query_as!(
                        BlockHeader,
                        r#"
                        SELECT
                            number as "number: _"
                            ,hash as "hash: _"
                            ,transactions_root as "transactions_root: _"
                            ,gas as "gas: _"
                            ,logs_bloom as "bloom: _"
                            ,timestamp_in_secs as "timestamp_in_secs: _"
                        FROM blocks
                        WHERE number = $1
                    "#,
                        block_number,
                    )
                    .fetch_one(&self.connection_pool);
                });

                todo!()
            }
            BlockSelection::Hash(hash) => {
                rt.block_on(async {
                    let header_query = sqlx::query_as!(
                        BlockHeader,
                        r#"
                        SELECT
                            number as "number: _"
                            ,hash as "hash: _"
                            ,transactions_root as "transactions_root: _"
                            ,gas as "gas: _"
                            ,logs_bloom as "bloom: _"
                            ,timestamp_in_secs as "timestamp_in_secs: _"
                        FROM blocks
                        WHERE hash = $1
                    "#,
                        hash.as_ref(),
                    )
                    .fetch_one(&self.connection_pool);

                    let transactions_query = sqlx::query_as!(
                        PostgresTransaction,
                        r#"
                        SELECT
                            hash as "hash: _"
                            ,signer_address as "signer_address: _"
                            ,nonce as "nonce: _"
                            ,address_from as "address_from: _"
                            ,address_to as "address_to: _"
                            ,input as "input: _"
                            ,gas as "gas: _"
                            ,idx_in_block as "idx_in_block: _"
                            ,block_number as "block_number: _"
                            ,block_hash as "block_hash: _"
                        FROM transactions
                        WHERE hash = $1
                        "#,
                        hash.as_ref()
                    )
                    .fetch_all(&self.connection_pool);

                    let logs_query = sqlx::query_as!(
                        PostgresLogs,
                        r#"
                        SELECT
                            address as "address: _"
                            ,data as "data: _"
                            ,transaction_hash as "transaction_hash: _"
                            ,transaction_idx as "transaction_idx: _"
                            ,log_idx as "log_idx: _"
                            ,block_number as "block_number: _"
                            ,block_hash as "block_hash: _"
                        FROM logs
                        WHERE block_hash = $1
                        "#,
                        hash.as_ref()
                    )
                    .fetch_all(&self.connection_pool);

                    let topics_query = sqlx::query_as!(
                        PostgresTopic,
                        r#"
                        SELECT
                            topic as "topic: _"
                            ,transaction_hash as "transaction_hash: _"
                            ,transaction_idx as "transaction_idx: _"
                            ,log_idx as "log_idx: _"
                            ,block_number as "block_number: _"
                            ,block_hash as "block_hash: _"
                        FROM topics
                        WHERE block_hash = $1
                        "#,
                        hash.as_ref()
                    )
                    .fetch_all(&self.connection_pool);

                    // run queries concurrently, but not in parallel
                    // see https://docs.rs/tokio/latest/tokio/macro.join.html#runtime-characteristics
                    let _res = tokio::join!(header_query, transactions_query, logs_query, topics_query);
                    // let header = res.0?;
                    // let transactions = res.1?;
                    // let logs = res.2?;
                    // let topics = res.3?;

                    // let block: Block = Block::new_with_capacity(BlockNumber::default(), 1, 1);

                    // let block = Block {
                    //     header,
                    //     transactions
                    // };

                    // Ok(block)
                });

                todo!()
            }
            BlockSelection::Number(number) => {
                let block_number = i64::try_from(*number).map_err(|_| EthError::StorageConvertError {
                    from: "BlockNumber".to_string(),
                    into: "i64".to_string(),
                })?;

                let _ = rt.block_on(async {
                    sqlx::query_as!(
                        BlockHeader,
                        r#"
                        SELECT
                            number as "number: _"
                            ,hash as "hash: _"
                            ,transactions_root as "transactions_root: _"
                            ,gas as "gas: _"
                            ,logs_bloom as "bloom: _"
                            ,timestamp_in_secs as "timestamp_in_secs: _"
                        FROM blocks
                        WHERE number = $1
                    "#,
                        block_number,
                    )
                    .fetch_one(&self.connection_pool)
                    .await
                });
            }
            BlockSelection::Earliest => {
                todo!()
            }
        };

        todo!();
    }

    fn read_mined_transaction(&self, hash: &Hash) -> Result<Option<TransactionMined>, EthError> {
        tracing::debug!(%hash, "reading transaction");
        todo!()
    }

    fn read_logs(&self, _: &LogFilter) -> Result<Vec<LogMined>, EthError> {
        Ok(Vec::new())
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
