use std::cell::Cell;
use std::collections::HashMap;
use std::mem::ManuallyDrop;
use std::time::Duration;

// use anyhow::anyhow;
use anyhow::Context;
use async_trait::async_trait;
use nonempty::nonempty;
use sqlx::pool::PoolConnection;
use sqlx::postgres::PgPoolOptions;
use sqlx::query_builder::QueryBuilder;
use sqlx::types::BigDecimal;
use sqlx::PgConnection;
use sqlx::PgPool;
use sqlx::Postgres;
use sqlx::Row;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockHeader;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
use crate::eth::primitives::ExecutionConflict;
use crate::eth::primitives::ExecutionConflicts;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Hash as TransactionHash;
use crate::eth::primitives::Log;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::LogTopic;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotIndexes;
use crate::eth::primitives::SlotSample;
use crate::eth::primitives::SlotValue;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::postgres_permanent::types::AccountBatch;
use crate::eth::storage::postgres_permanent::types::HistoricalBalanceBatch;
use crate::eth::storage::postgres_permanent::types::HistoricalNonceBatch;
use crate::eth::storage::postgres_permanent::types::HistoricalSlotBatch;
use crate::eth::storage::postgres_permanent::types::LogBatch;
use crate::eth::storage::postgres_permanent::types::PostgresLog;
use crate::eth::storage::postgres_permanent::types::PostgresTransaction;
use crate::eth::storage::postgres_permanent::types::SlotBatch;
use crate::eth::storage::postgres_permanent::types::TransactionBatch;
use crate::eth::storage::PermanentStorage;
use crate::eth::storage::StorageError;
use crate::log_and_err;

#[derive(Debug)]
pub struct PostgresPermanentStorageConfig {
    pub url: String,
    pub connections: u32,
    pub acquire_timeout: Duration,
}

pub struct PostgresPermanentStorage {
    pub pool: PgPool,
}

impl Drop for PostgresPermanentStorage {
    fn drop(&mut self) {
        THREAD_CONN.take();
    }
}

impl PostgresPermanentStorage {
    /// Creates a new [`PostgresPermanentStorage`].
    pub async fn new(config: PostgresPermanentStorageConfig) -> anyhow::Result<Self> {
        tracing::info!(?config, "starting postgres permanent storage");

        let result = PgPoolOptions::new()
            .min_connections(config.connections)
            .max_connections(config.connections)
            .acquire_timeout(config.acquire_timeout)
            .connect(&config.url)
            .await;

        let pool = match result {
            Ok(pool) => pool.clone(),
            Err(e) => return log_and_err!(reason = e, "failed to start postgres permanent storage"),
        };

        let storage = Self { pool };

        Ok(storage)
    }
}

#[async_trait]
impl PermanentStorage for PostgresPermanentStorage {
    async fn allocate_evm_thread_resources(&self) -> anyhow::Result<()> {
        let conn = self.pool.acquire().await?;
        let conn = conn.leak();
        THREAD_CONN.set(Some(conn));
        Ok(())
    }

    async fn increment_block_number(&self) -> anyhow::Result<BlockNumber> {
        tracing::debug!("incrementing block number");

        let nextval = sqlx::query_file_scalar!("src/eth/storage/postgres_permanent/sql/select_current_block_number.sql")
            .fetch_one(&self.pool)
            .await
            .unwrap_or_else(|err| {
                tracing::error!(?err, "failed to get block number");
                BigDecimal::from(0)
            })
            + BigDecimal::from(1);

        nextval.try_into()
    }

    async fn set_mined_block_number(&self, _: BlockNumber) -> anyhow::Result<()> {
        // nothing to do yet because we are not using a sequence
        Ok(())
    }

    async fn read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Account>> {
        tracing::debug!(%address, "reading account");

        let mut conn = PoolOrThreadConnection::take(&self.pool).await?;
        let account = match point_in_time {
            StoragePointInTime::Present => {
                // We have to get the account information closest to the block with the given block_number
                sqlx::query_file_as!(Account, "src/eth/storage/postgres_permanent/sql/select_account.sql", address.as_ref())
                    .fetch_optional(conn.for_sqlx())
                    .await?
            }
            StoragePointInTime::Past(number) => {
                let block_number: i64 = (*number).try_into()?;
                // We have to get the account information closest to the block with the given block_number
                sqlx::query_file_as!(
                    Account,
                    "src/eth/storage/postgres_permanent/sql/select_account_at_block.sql",
                    address.as_ref(),
                    block_number as _,
                )
                .fetch_optional(conn.for_sqlx())
                .await?
            }
        };

        match account {
            Some(account) => {
                tracing::trace!(%address, ?account, "account found");
                Ok(Some(account))
            }
            None => {
                tracing::trace!(%address, "account not found");
                Ok(None)
            }
        }
    }

    async fn read_slot(&self, address: &Address, index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>> {
        tracing::debug!(%address, %index, "reading slot");

        // TODO: improve this conversion
        let slot_index_u8: [u8; 32] = index.clone().into();

        let mut conn = PoolOrThreadConnection::take(&self.pool).await?;
        let slot_value_vec: Option<Vec<u8>> = match point_in_time {
            StoragePointInTime::Present =>
                sqlx::query_file_scalar!(
                    "src/eth/storage/postgres_permanent/sql/select_slot.sql",
                    address.as_ref(),
                    slot_index_u8.as_ref(),
                )
                .fetch_optional(conn.for_sqlx())
                .await?,
            StoragePointInTime::Past(number) => {
                let block_number: i64 = (*number).try_into()?;
                sqlx::query_file_scalar!(
                    "src/eth/storage/postgres_permanent/sql/select_slot_at_block.sql",
                    address.as_ref(),
                    slot_index_u8.as_ref(),
                    block_number as _,
                )
                .fetch_optional(conn.for_sqlx())
                .await?
            }
        };

        // If there is no slot, we return
        // an "empty slot"
        match slot_value_vec {
            Some(slot_value_vec) => {
                let slot_value = SlotValue::from(slot_value_vec);
                let slot = Slot {
                    index: index.clone(),
                    value: slot_value,
                };
                tracing::trace!(?address, ?index, %slot, "slot found");
                Ok(Some(slot))
            }
            None => {
                tracing::trace!(?address, ?index, ?point_in_time, "slot not found");
                Ok(None)
            }
        }
    }

    async fn read_slots(&self, address: &Address, indexes: &[SlotIndex], point_in_time: &StoragePointInTime) -> anyhow::Result<HashMap<SlotIndex, SlotValue>> {
        tracing::debug!(%address, indexes_len = %indexes.len(), "reading slots");

        let slots = match point_in_time {
            StoragePointInTime::Present =>
                sqlx::query_file_as!(Slot, "src/eth/storage/postgres_permanent/sql/select_slots.sql", indexes as _, address as _)
                    .fetch_all(&self.pool)
                    .await?,
            StoragePointInTime::Past(block_number) =>
                sqlx::query_file_as!(
                    Slot,
                    "src/eth/storage/postgres_permanent/sql/select_historical_slots.sql",
                    indexes as _,
                    address as _,
                    block_number as _
                )
                .fetch_all(&self.pool)
                .await?,
        };
        Ok(slots.into_iter().map(|slot| (slot.index, slot.value)).collect())
    }

    async fn read_block(&self, block: &BlockSelection) -> anyhow::Result<Option<Block>> {
        tracing::debug!(block = ?block, "reading block");

        match block {
            BlockSelection::Latest => {
                let current = self.read_mined_block_number().await?;

                let block_number = i64::try_from(current)?;

                let header_query = sqlx::query_file_as!(
                    BlockHeader,
                    "src/eth/storage/postgres_permanent/sql/select_block_header_by_number.sql",
                    block_number as _
                )
                .fetch_optional(&self.pool);

                let transactions_query = sqlx::query_file_as!(
                    PostgresTransaction,
                    "src/eth/storage/postgres_permanent/sql/select_transactions_by_block_number.sql",
                    block_number as _
                )
                .fetch_all(&self.pool);

                let logs_query = sqlx::query_file_as!(
                    PostgresLog,
                    "src/eth/storage/postgres_permanent/sql/select_logs_by_block_number.sql",
                    block_number as _
                )
                .fetch_all(&self.pool);

                // run queries concurrently, but not in parallel
                // see https://docs.rs/tokio/latest/tokio/macro.join.html#runtime-characteristics
                let res = tokio::join!(header_query, transactions_query, logs_query);
                let header = match res.0 {
                    Ok(Some(header)) => header,
                    Ok(None) => return Ok(None),
                    Err(e) => return log_and_err!(reason = e, "failed to query block by latest"),
                };
                let transactions = res.1?;
                let logs = res.2?;

                let mut log_partitions = partition_logs(logs);

                let transactions = transactions
                    .into_iter()
                    .map(|tx| {
                        let this_tx_logs = log_partitions.remove(&tx.hash).unwrap_or_default();
                        tx.into_transaction_mined(this_tx_logs)
                    })
                    .collect();

                let block = Block { header, transactions };

                Ok::<Option<Block>, anyhow::Error>(Some(block))
            }

            BlockSelection::Hash(hash) => {
                let header_query = sqlx::query_file_as!(
                    BlockHeader,
                    "src/eth/storage/postgres_permanent/sql/select_block_header_by_hash.sql",
                    hash.as_ref(),
                )
                .fetch_optional(&self.pool);

                let transactions_query = sqlx::query_file_as!(
                    PostgresTransaction,
                    "src/eth/storage/postgres_permanent/sql/select_transactions_by_block_hash.sql",
                    hash.as_ref()
                )
                .fetch_all(&self.pool);

                let logs_query = sqlx::query_file_as!(
                    PostgresLog,
                    "src/eth/storage/postgres_permanent/sql/select_logs_by_block_hash.sql",
                    hash.as_ref()
                )
                .fetch_all(&self.pool);

                // run queries concurrently, but not in parallel
                // see https://docs.rs/tokio/latest/tokio/macro.join.html#runtime-characteristics
                let res = tokio::join!(header_query, transactions_query, logs_query);
                let header = match res.0 {
                    Ok(Some(header)) => header,
                    Ok(None) => return Ok(None),
                    Err(e) => return log_and_err!(reason = e, "failed to query block by hash"),
                };
                let transactions = res.1?;
                let logs = res.2?;

                let mut log_partitions = partition_logs(logs);

                let transactions = transactions
                    .into_iter()
                    .map(|tx| {
                        let this_tx_logs = log_partitions.remove(&tx.hash).unwrap_or_default();
                        tx.into_transaction_mined(this_tx_logs)
                    })
                    .collect();

                let block = Block { header, transactions };

                Ok::<Option<Block>, anyhow::Error>(Some(block))
            }

            BlockSelection::Number(number) => {
                let block_number = i64::try_from(*number)?;

                let header_query = sqlx::query_file_as!(
                    BlockHeader,
                    "src/eth/storage/postgres_permanent/sql/select_block_header_by_number.sql",
                    block_number as _
                )
                .fetch_optional(&self.pool);

                let transactions_query = sqlx::query_file_as!(
                    PostgresTransaction,
                    "src/eth/storage/postgres_permanent/sql/select_transactions_by_block_number.sql",
                    block_number as _
                )
                .fetch_all(&self.pool);

                let logs_query = sqlx::query_file_as!(
                    PostgresLog,
                    "src/eth/storage/postgres_permanent/sql/select_logs_by_block_number.sql",
                    block_number as _
                )
                .fetch_all(&self.pool);

                // run queries concurrently, but not in parallel
                // see https://docs.rs/tokio/latest/tokio/macro.join.html#runtime-characteristics
                let res = tokio::join!(header_query, transactions_query, logs_query);
                let header = match res.0 {
                    Ok(Some(header)) => header,
                    Ok(None) => return Ok(None),
                    Err(e) => return log_and_err!(reason = e, "failed to query block by number"),
                };
                let transactions = res.1?;
                let logs = res.2?;

                let mut log_partitions = partition_logs(logs);

                let transactions = transactions
                    .into_iter()
                    .map(|tx| {
                        let this_tx_logs = log_partitions.remove(&tx.hash).unwrap_or_default();
                        tx.into_transaction_mined(this_tx_logs)
                    })
                    .collect();

                let block = Block { header, transactions };

                Ok::<Option<Block>, anyhow::Error>(Some(block))
            }
            BlockSelection::Earliest => {
                let block_number = 0i64;

                let header_query = sqlx::query_file_as!(
                    BlockHeader,
                    "src/eth/storage/postgres_permanent/sql/select_block_header_by_number.sql",
                    block_number as _
                )
                .fetch_optional(&self.pool);

                let transactions_query = sqlx::query_file_as!(
                    PostgresTransaction,
                    "src/eth/storage/postgres_permanent/sql/select_transactions_by_block_number.sql",
                    block_number as _
                )
                .fetch_all(&self.pool);

                let logs_query = sqlx::query_file_as!(
                    PostgresLog,
                    "src/eth/storage/postgres_permanent/sql/select_logs_by_block_number.sql",
                    block_number as _
                )
                .fetch_all(&self.pool);

                // run queries concurrently, but not in parallel
                // see https://docs.rs/tokio/latest/tokio/macro.join.html#runtime-characteristics
                let res = tokio::join!(header_query, transactions_query, logs_query);
                let header = match res.0 {
                    Ok(Some(header)) => header,
                    Ok(None) => return Ok(None),
                    Err(e) => return log_and_err!(reason = e, "failed to query block by earlist"),
                };
                let transactions = res.1?;
                let logs = res.2?;

                let mut log_partitions = partition_logs(logs);

                let transactions = transactions
                    .into_iter()
                    .map(|tx| {
                        let this_tx_logs = log_partitions.remove(&tx.hash).unwrap();
                        tx.into_transaction_mined(this_tx_logs)
                    })
                    .collect();

                let block = Block { header, transactions };

                Ok::<Option<Block>, anyhow::Error>(Some(block))
            }
        }
    }

    async fn read_mined_transaction(&self, hash: &Hash) -> anyhow::Result<Option<TransactionMined>> {
        tracing::debug!(%hash, "reading transaction");
        let transaction_query = sqlx::query_file_as!(
            PostgresTransaction,
            "src/eth/storage/postgres_permanent/sql/select_transaction_by_hash.sql",
            hash.as_ref()
        )
        .fetch_one(&self.pool)
        .await;

        let transaction: PostgresTransaction = match transaction_query {
            Ok(res) => res,
            Err(sqlx::Error::RowNotFound) => return Ok(None),
            err => err?,
        };

        let logs = sqlx::query_file_as!(
            PostgresLog,
            "src/eth/storage/postgres_permanent/sql/select_logs_by_transaction_hash.sql",
            hash.as_ref()
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(Some(transaction.into_transaction_mined(logs)))
    }

    async fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        tracing::debug!(filter = ?filter, "Reading logs");
        let from: i64 = filter.from_block.try_into()?;
        let query = include_str!("sql/select_logs.sql");

        let log_query_builder = &mut QueryBuilder::new(query);
        log_query_builder.push(" AND block_number >= ");
        log_query_builder.push_bind(from);

        // verifies if to_block exists
        if let Some(block_number) = filter.to_block {
            log_query_builder.push(" AND block_number <= ");
            let to: i64 = block_number.try_into()?;
            log_query_builder.push_bind(to);
        }

        let log_query = log_query_builder.build();

        let query_result = log_query.fetch_all(&self.pool).await?;

        let mut result = vec![];

        for row in query_result {
            let block_hash: &[u8] = row.get("block_hash");
            let log_idx: BigDecimal = row.get("log_idx");

            let logs = sqlx::query_file_as!(
                PostgresLog,
                "src/eth/storage/postgres_permanent/sql/select_logs_by_block_hash_log_idx.sql",
                block_hash,
                log_idx as _
            )
            .fetch_all(&self.pool)
            .await?; // OPTIMIZE: We should query for the logs only once

            let topics = logs.iter().flat_map(PostgresLog::to_topics);

            let log = LogMined {
                log: Log {
                    address: row.get("address"),
                    data: row.get("data"),
                    topics: topics.map(LogTopic::from).collect(),
                },
                transaction_hash: row.get("transaction_hash"),
                transaction_index: row.get("transaction_idx"),
                log_index: row.get("log_idx"),
                block_number: row.get("block_number"),
                block_hash: row.get("block_hash"),
            };
            result.push(log);
        }

        tracing::debug!(logs = ?result, "Read logs");
        Ok(result.into_iter().filter(|log| filter.matches(log)).collect())
    }

    // TODO: It might be possible to either:
    //        1- Aggregate all queries in a single network call
    //        2- Get the futures for the queries and run them concurrently.
    // The second approach would be easy to implement if we began more than one transaction,
    // and we relaxed the relations in the database but we want everything to happen
    // in only one transaction so that we can rollback in case of conflicts.
    // The first would be easy if sqlx supported pipelining  (https://github.com/launchbadge/sqlx/issues/408)
    // like tokio_postgres does https://docs.rs/tokio-postgres/0.4.0-rc.3/tokio_postgres/#pipelining
    async fn save_block(&self, block: Block) -> anyhow::Result<(), StorageError> {
        tracing::debug!(block = ?block, "saving block");

        let account_changes = block.compact_account_changes();

        let mut transaction_batch = TransactionBatch::default();
        let mut log_batch = LogBatch::default();
        let mut account_batch = AccountBatch::default();
        let mut historical_nonce_batch = HistoricalNonceBatch::default();
        let mut historical_balance_batch = HistoricalBalanceBatch::default();
        let mut slot_batch = SlotBatch::default();
        let mut historical_slot_batch = HistoricalSlotBatch::default();

        for mut transaction in block.transactions {
            let is_success = transaction.is_success();
            let logs = std::mem::take(&mut transaction.logs);
            transaction_batch.push(transaction);

            if is_success {
                for log in logs {
                    log_batch.push(log);
                }
            }
        }

        let mut sload_batch = vec![];
        for change in account_changes {
            // for change in transaction.execution.changes {
            let (original_nonce, new_nonce) = change.nonce.take_both();
            let (original_balance, new_balance) = change.balance.take_both();

            let original_nonce = original_nonce.unwrap_or_default();
            let original_balance = original_balance.unwrap_or_default();

            let bytecode = change.bytecode.take().unwrap_or_else(|| {
                tracing::debug!("bytecode not set, defaulting to None");
                None
            });

            let static_slot_indexes = change.static_slot_indexes.take().unwrap_or_default();
            let mapping_slot_indexes = change.mapping_slot_indexes.take().unwrap_or_default();

            account_batch.push(
                change.address.clone(),
                new_nonce.clone().unwrap_or(original_nonce.clone()),
                new_balance.clone().unwrap_or(original_balance.clone()),
                bytecode,
                block.header.number,
                original_nonce,
                original_balance,
                change.code_hash,
                static_slot_indexes,
                mapping_slot_indexes,
            );

            if let Some(balance) = new_balance {
                historical_balance_batch.push(change.address.clone(), balance, block.header.number);
            }

            if let Some(nonce) = new_nonce {
                historical_nonce_batch.push(change.address.clone(), nonce, block.header.number);
            }

            for (slot_idx, value) in change.slots {
                let (original_value, val) = value.clone().take_both();

                let new_value = match val {
                    Some(s) => s.value,
                    None => {
                        tracing::trace!("slot value not set, skipping");
                        continue;
                    }
                };
                let original_value = original_value.unwrap_or_default().value;

                slot_batch.push(change.address.clone(), slot_idx.clone(), new_value.clone(), block.header.number, original_value);
                historical_slot_batch.push(change.address.clone(), slot_idx.clone(), new_value.clone(), block.header.number);

                sload_batch.push((change.address.clone(), slot_idx.clone(), new_value.clone(), block.header.number));
            }
        }

        let expected_modified_slots = slot_batch.address.len();
        let expected_modified_accounts = account_batch.address.len();

        let mut tx = self.pool.begin().await.context("failed to init save_block transaction")?;

        let block_result = sqlx::query_file!(
            "src/eth/storage/postgres_permanent/sql/insert_entire_block.sql",
            block.header.number as _,                                                                 // $1
            block.header.hash.as_ref(),                                                               // $2
            block.header.transactions_root.as_ref(),                                                  // $3
            block.header.gas_limit as _,                                                              // $4
            block.header.gas_used as _,                                                               // $5
            block.header.bloom.as_ref(),                                                              // $6
            i64::try_from(block.header.timestamp).context("failed to convert block timestamp")? as _, // $7
            block.header.parent_hash.as_ref(),                                                        // $8
            block.header.author as _,                                                                 // $9
            block.header.extra_data as _,                                                             // $10
            block.header.miner as _,                                                                  // $11
            block.header.difficulty as _,                                                             // $12
            block.header.receipts_root as _,                                                          // $13
            block.header.uncle_hash as _,                                                             // $14
            block.header.size as _,                                                                   // $15
            block.header.state_root as _,                                                             // $16
            block.header.total_difficulty as _,                                                       // $17
            block.header.nonce as _,                                                                  // $18
            transaction_batch.hash as _,                                                              // $19
            transaction_batch.signer as _,                                                            // $20
            transaction_batch.nonce as _,                                                             // $21
            transaction_batch.from as _,                                                              // $22
            transaction_batch.to as _,                                                                // $23
            transaction_batch.input as _,                                                             // $24
            transaction_batch.output as _,                                                            // $25
            transaction_batch.gas as _,                                                               // $26
            transaction_batch.gas_price as _,                                                         // $27
            transaction_batch.index as _,                                                             // $28
            transaction_batch.block_number as _,                                                      // $29
            transaction_batch.block_hash as _,                                                        // $30
            transaction_batch.v as _,                                                                 // $31
            transaction_batch.r as _,                                                                 // $32
            transaction_batch.s as _,                                                                 // $33
            transaction_batch.value as _,                                                             // $34
            &transaction_batch.result,                                                                // $35
            transaction_batch.chain_id as _,                                                          // $36
            log_batch.address as _,                                                                   // $37
            log_batch.data as _,                                                                      // $38
            log_batch.transaction_hash as _,                                                          // $39
            log_batch.transaction_index as _,                                                         // $40
            log_batch.log_index as _,                                                                 // $41
            log_batch.block_number as _,                                                              // $42
            log_batch.block_hash as _,                                                                // $43
            log_batch.topic0 as _,                                                                    // $44
            log_batch.topic1 as _,                                                                    // $45
            log_batch.topic2 as _,                                                                    // $46
            log_batch.topic3 as _,                                                                    // $47
            account_batch.address as _,                                                               // $48
            account_batch.bytecode as _,                                                              // $49
            account_batch.new_balance as _,                                                           // $50
            account_batch.new_nonce as _,                                                             // $51
            account_batch.block_number as _,                                                          // $52
            account_batch.original_balance as _,                                                      // $53
            account_batch.original_nonce as _,                                                        // $54
            account_batch.code_hash as _,                                                             // $55
            account_batch.static_slot_indexes as _,                                                   // $56
            account_batch.mapping_slot_indexes as _,                                                  // $57
            slot_batch.index as _,                                                                    // $58
            slot_batch.value as _,                                                                    // $59
            slot_batch.address as _,                                                                  // $60
            slot_batch.block_number as _,                                                             // $61
            slot_batch.original_value as _,                                                           // $62
            historical_nonce_batch.address as _,                                                      // $63
            historical_nonce_batch.nonce as _,                                                        // $64
            historical_nonce_batch.block_number as _,                                                 // $65
            historical_balance_batch.address as _,                                                    // $66
            historical_balance_batch.balance as _,                                                    // $67
            historical_balance_batch.block_number as _,                                               // $68
            historical_slot_batch.index as _,                                                         // $69
            historical_slot_batch.value as _,                                                         // $70
            historical_slot_batch.address as _,                                                       // $71
            historical_slot_batch.block_number as _,                                                  // $72
        )
        .fetch_one(&mut *tx)
        .await
        .context("failed to insert block")?;

        let modified_accounts = block_result.modified_accounts.unwrap_or_default() as usize;
        let modified_slots = block_result.modified_slots.unwrap_or_default() as usize;

        if modified_accounts != expected_modified_accounts {
            tx.rollback().await.context("failed to rollback transaction")?;
            let error: StorageError = StorageError::Conflict(ExecutionConflicts(nonempty![ExecutionConflict::Account]));
            return Err(error);
        }

        if modified_slots != expected_modified_slots {
            tx.rollback().await.context("failed to rollback transaction")?;
            let error: StorageError = StorageError::Conflict(ExecutionConflicts(nonempty![ExecutionConflict::PgSlot]));
            return Err(error);
        }

        tx.commit().await.context("failed to commit transaction")?;

        Ok(())
    }

    async fn read_mined_block_number(&self) -> anyhow::Result<BlockNumber> {
        tracing::debug!("reading current block number");

        let currval: BigDecimal = sqlx::query_file_scalar!("src/eth/storage/postgres_permanent/sql/select_current_block_number.sql")
            .fetch_one(&self.pool)
            .await
            .unwrap_or(BigDecimal::from(0));

        currval.try_into()
    }

    async fn save_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<()> {
        tracing::debug!(?accounts, "saving initial accounts");

        for acc in accounts {
            let mut tx = self.pool.begin().await.context("failed to init transaction")?;
            let block_number = 0;
            let balance = BigDecimal::try_from(acc.balance)?;
            let nonce = BigDecimal::try_from(acc.nonce)?;
            let bytecode = acc.bytecode.as_deref();
            let code_hash: &[u8] = acc.code_hash.as_ref();
            let static_slot_indexes: Option<SlotIndexes> = acc.static_slot_indexes;
            let mapping_slot_indexes: Option<SlotIndexes> = acc.mapping_slot_indexes;

            sqlx::query_file!(
                "src/eth/storage/postgres_permanent/sql/insert_account.sql",
                acc.address.as_ref(),
                nonce,
                balance,
                bytecode,
                code_hash,
                static_slot_indexes as _,
                mapping_slot_indexes as _,
                block_number as _,
                BigDecimal::from(0),
                BigDecimal::from(0),
            )
            .execute(&mut *tx)
            .await
            .context("failed to insert account")?;

            sqlx::query_file!(
                "src/eth/storage/postgres_permanent/sql/insert_historical_balance.sql",
                acc.address.as_ref(),
                balance,
                block_number as _
            )
            .execute(&mut *tx)
            .await
            .context("failed to insert balance")?;

            sqlx::query_file!(
                "src/eth/storage/postgres_permanent/sql/insert_historical_nonce.sql",
                acc.address.as_ref(),
                nonce,
                block_number as _
            )
            .execute(&mut *tx)
            .await
            .context("failed to insert nonce")?;

            tx.commit().await.context("failed to commit transaction")?;
        }

        Ok(())
    }

    async fn reset_at(&self, number: BlockNumber) -> anyhow::Result<()> {
        sqlx::query_file!("src/eth/storage/postgres_permanent/sql/delete_after_block.sql", number as _)
            .execute(&self.pool)
            .await?;

        // Rollback the values of account.latest_balance, account.latest_nonce and
        // account_slots.value.

        sqlx::query_file!("src/eth/storage/postgres_permanent/sql/update_account_reset_balance.sql")
            .execute(&self.pool)
            .await?;

        sqlx::query_file!("src/eth/storage/postgres_permanent/sql/update_account_reset_nonce.sql")
            .execute(&self.pool)
            .await?;

        sqlx::query_file!("src/eth/storage/postgres_permanent/sql/update_account_slots_reset_value.sql")
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn read_slots_sample(&self, start: BlockNumber, end: BlockNumber, max_samples: u64, seed: u64) -> anyhow::Result<Vec<SlotSample>> {
        let seed = (seed as f64 / 100.0).fract();
        let max_samples: Option<i64> = match max_samples {
            0 => None,
            n => Some(n as i64),
        };

        let slots_sample_rows = sqlx::query_file_as!(
            SlotSample,
            "src/eth/storage/postgres_permanent/sql/select_random_slot_sample.sql",
            seed,
            start as _,
            end as _,
            max_samples
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(slots_sample_rows)
    }
}

fn partition_logs(logs: impl IntoIterator<Item = PostgresLog>) -> HashMap<TransactionHash, Vec<PostgresLog>> {
    let mut partitions: HashMap<TransactionHash, Vec<PostgresLog>> = HashMap::new();
    for log in logs {
        if let Some(part) = partitions.get_mut(&log.transaction_hash) {
            part.push(log);
        } else {
            partitions.insert(log.transaction_hash.clone(), vec![log]);
        }
    }
    partitions
}

// -----------------------------------------------------------------------------
// TODO: move elsewhere if neeeded for other modules that uses PostgreSQL.
// -----------------------------------------------------------------------------
thread_local! {
    pub static THREAD_CONN: Cell<Option<PgConnection>> = const { Cell::new(None) };
}

/// A Postgres connection acquired from the pool of connections or the current thread-local storage.
///
/// When dropped, it will be automatically returned to the pool or to the thread-local storage according to its origin.
struct PoolOrThreadConnection(PoolOrThreadConnectionKind);

impl Drop for PoolOrThreadConnection {
    fn drop(&mut self) {
        if let PoolOrThreadConnectionKind::Leaked(ref mut conn) = self.0 {
            let conn = unsafe { ManuallyDrop::take(conn) };
            THREAD_CONN.set(Some(conn));
        }
    }
}

impl PoolOrThreadConnection {
    /// Takes ownership of a connection according to the current thread availability.
    async fn take(pool: &PgPool) -> anyhow::Result<Self> {
        match THREAD_CONN.take() {
            Some(conn) => Ok(Self(PoolOrThreadConnectionKind::Leaked(ManuallyDrop::new(conn)))),
            None => Ok(Self(PoolOrThreadConnectionKind::Managed(pool.acquire().await?))),
        }
    }

    /// Casts the current connection for SQLx usage.
    fn for_sqlx(&mut self) -> &mut PgConnection {
        match &mut self.0 {
            PoolOrThreadConnectionKind::Leaked(ref mut conn) => conn,
            PoolOrThreadConnectionKind::Managed(conn) => conn.as_mut(),
        }
    }
}

enum PoolOrThreadConnectionKind {
    /// Connection removed from SQLx pool.
    Leaked(ManuallyDrop<PgConnection>),

    /// Connection managed by SQLx pool.
    Managed(PoolConnection<Postgres>),
}
