use std::collections::HashMap;

use anyhow::Context;
use async_trait::async_trait;
use nonempty::nonempty;
use sqlx::query_builder::QueryBuilder;
use sqlx::types::BigDecimal;
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
use crate::eth::primitives::Index as LogIndex;
use crate::eth::primitives::Log;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::LogTopic;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotSample;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::postgres::types::AccountBatch;
use crate::eth::storage::postgres::types::HistoricalBalanceBatch;
use crate::eth::storage::postgres::types::HistoricalNonceBatch;
use crate::eth::storage::postgres::types::HistoricalSlotBatch;
use crate::eth::storage::postgres::types::LogBatch;
use crate::eth::storage::postgres::types::PostgresLog;
use crate::eth::storage::postgres::types::PostgresTopic;
use crate::eth::storage::postgres::types::PostgresTransaction;
use crate::eth::storage::postgres::types::SlotBatch;
use crate::eth::storage::postgres::types::TopicBatch;
use crate::eth::storage::postgres::types::TransactionBatch;
use crate::eth::storage::PermanentStorage;
use crate::eth::storage::StorageError;
use crate::infra::postgres::Postgres;
use crate::log_and_err;

#[async_trait]
impl PermanentStorage for Postgres {
    async fn increment_block_number(&self) -> anyhow::Result<BlockNumber> {
        tracing::debug!("incrementing block number");

        let nextval: i64 = sqlx::query_file_scalar!("src/eth/storage/postgres/queries/select_current_block_number.sql")
            .fetch_one(&self.connection_pool)
            .await
            .unwrap_or_else(|err| {
                tracing::error!(?err, "failed to get block number");
                0
            })
            + 1;

        Ok(nextval.into())
    }

    async fn set_block_number(&self, _: BlockNumber) -> anyhow::Result<()> {
        // nothing to do yet because we are not using a sequence
        Ok(())
    }

    async fn maybe_read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Account>> {
        tracing::debug!(%address, "reading account");
        let account = match point_in_time {
            StoragePointInTime::Present => {
                // We have to get the account information closest to the block with the given block_number
                sqlx::query_file_as!(Account, "src/eth/storage/postgres/queries/select_account.sql", address.as_ref())
                    .fetch_optional(&self.connection_pool)
                    .await?
            }
            StoragePointInTime::Past(number) => {
                let block_number: i64 = (*number).try_into()?;
                // We have to get the account information closest to the block with the given block_number
                sqlx::query_file_as!(
                    Account,
                    "src/eth/storage/postgres/queries/select_account_at_block.sql",
                    address.as_ref(),
                    block_number,
                )
                .fetch_optional(&self.connection_pool)
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

    async fn maybe_read_slot(&self, address: &Address, slot_index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>> {
        tracing::debug!(%address, %slot_index, "reading slot");

        // TODO: improve this conversion
        let slot_index: [u8; 32] = slot_index.clone().into();

        let slot = match point_in_time {
            StoragePointInTime::Present => {
                sqlx::query_file_as!(Slot, "src/eth/storage/postgres/queries/select_slot.sql", address.as_ref(), slot_index.as_ref())
                    .fetch_optional(&self.connection_pool)
                    .await?
            }
            StoragePointInTime::Past(number) => {
                let block_number: i64 = (*number).try_into()?;
                sqlx::query_file_as!(
                    Slot,
                    "src/eth/storage/postgres/queries/select_slot_at_block.sql",
                    address.as_ref(),
                    slot_index.as_ref(),
                    block_number,
                )
                .fetch_optional(&self.connection_pool)
                .await?
            }
        };

        // If there is no slot, we return
        // an "empty slot"
        match slot {
            Some(slot) => {
                tracing::trace!(?address, ?slot_index, %slot, "slot found");
                Ok(Some(slot))
            }
            None => {
                tracing::trace!(?address, ?slot_index, ?point_in_time, "slot not found");
                Ok(None)
            }
        }
    }

    async fn read_block(&self, block: &BlockSelection) -> anyhow::Result<Option<Block>> {
        tracing::debug!(block = ?block, "reading block");

        match block {
            BlockSelection::Latest => {
                let current = self.read_current_block_number().await?;

                let block_number = i64::try_from(current)?;

                let header_query = sqlx::query_file_as!(BlockHeader, "src/eth/storage/postgres/queries/select_block_header_by_number.sql", block_number,)
                    .fetch_optional(&self.connection_pool);

                let transactions_query = sqlx::query_file_as!(
                    PostgresTransaction,
                    "src/eth/storage/postgres/queries/select_transactions_by_block_number.sql",
                    block_number
                )
                .fetch_all(&self.connection_pool);

                let logs_query = sqlx::query_file_as!(PostgresLog, "src/eth/storage/postgres/queries/select_logs_by_block_number.sql", block_number)
                    .fetch_all(&self.connection_pool);

                let topics_query = sqlx::query_file_as!(
                    PostgresTopic,
                    "src/eth/storage/postgres/queries/select_topics_by_block_number.sql",
                    block_number
                )
                .fetch_all(&self.connection_pool);

                // run queries concurrently, but not in parallel
                // see https://docs.rs/tokio/latest/tokio/macro.join.html#runtime-characteristics
                let res = tokio::join!(header_query, transactions_query, logs_query, topics_query);
                let header = match res.0 {
                    Ok(Some(header)) => header,
                    Ok(None) => return Ok(None),
                    Err(e) => return log_and_err!(reason = e, "failed to query block by latest"),
                };
                let transactions = res.1?;
                let logs = res.2?.into_iter();
                let topics = res.3?.into_iter();

                // We're still cloning the hashes, maybe create a HashMap structure like this
                // `HashMap<PostgresTransaction, Vec<HashMap<PostgresLog, Vec<PostgresTopic>>>>` in the future
                // so that we don't have to clone the hashes
                let mut log_partitions = partition_logs(logs);
                let mut topic_partitions = partition_topics(topics);
                let transactions = transactions
                    .into_iter()
                    .map(|tx| {
                        let this_tx_logs = log_partitions.remove(&tx.hash).unwrap_or_default();
                        let this_tx_topics = topic_partitions.remove(&tx.hash).unwrap_or_default();
                        tx.into_transaction_mined(this_tx_logs, this_tx_topics)
                    })
                    .collect();

                let block = Block { header, transactions };

                Ok::<Option<Block>, anyhow::Error>(Some(block))
            }

            BlockSelection::Hash(hash) => {
                let header_query = sqlx::query_file_as!(BlockHeader, "src/eth/storage/postgres/queries/select_block_header_by_hash.sql", hash.as_ref(),)
                    .fetch_optional(&self.connection_pool);

                let transactions_query = sqlx::query_file_as!(
                    PostgresTransaction,
                    "src/eth/storage/postgres/queries/select_transactions_by_block_hash.sql",
                    hash.as_ref()
                )
                .fetch_all(&self.connection_pool);

                let logs_query = sqlx::query_file_as!(PostgresLog, "src/eth/storage/postgres/queries/select_logs_by_block_hash.sql", hash.as_ref())
                    .fetch_all(&self.connection_pool);

                let topics_query = sqlx::query_file_as!(PostgresTopic, "src/eth/storage/postgres/queries/select_topics_by_block_hash.sql", hash.as_ref())
                    .fetch_all(&self.connection_pool);

                // run queries concurrently, but not in parallel
                // see https://docs.rs/tokio/latest/tokio/macro.join.html#runtime-characteristics
                let res = tokio::join!(header_query, transactions_query, logs_query, topics_query);
                let header = match res.0 {
                    Ok(Some(header)) => header,
                    Ok(None) => return Ok(None),
                    Err(e) => return log_and_err!(reason = e, "failed to query block by hash"),
                };
                let transactions = res.1?;
                let logs = res.2?.into_iter();
                let topics = res.3?.into_iter();

                // We're still cloning the hashes, maybe create a HashMap structure like this
                // `HashMap<PostgresTransaction, Vec<HashMap<PostgresLog, Vec<PostgresTopic>>>>` in the future
                // so that we don't have to clone the hashes
                let mut log_partitions = partition_logs(logs);
                let mut topic_partitions = partition_topics(topics);
                let transactions = transactions
                    .into_iter()
                    .map(|tx| {
                        let this_tx_logs = log_partitions.remove(&tx.hash).unwrap_or_default();
                        let this_tx_topics = topic_partitions.remove(&tx.hash).unwrap_or_default();
                        tx.into_transaction_mined(this_tx_logs, this_tx_topics)
                    })
                    .collect();

                let block = Block { header, transactions };

                Ok::<Option<Block>, anyhow::Error>(Some(block))
            }

            BlockSelection::Number(number) => {
                let block_number = i64::try_from(*number)?;

                let header_query = sqlx::query_file_as!(BlockHeader, "src/eth/storage/postgres/queries/select_block_header_by_number.sql", block_number,)
                    .fetch_optional(&self.connection_pool);

                let transactions_query = sqlx::query_file_as!(
                    PostgresTransaction,
                    "src/eth/storage/postgres/queries/select_transactions_by_block_number.sql",
                    block_number
                )
                .fetch_all(&self.connection_pool);

                let logs_query = sqlx::query_file_as!(PostgresLog, "src/eth/storage/postgres/queries/select_logs_by_block_number.sql", block_number)
                    .fetch_all(&self.connection_pool);

                let topics_query = sqlx::query_file_as!(
                    PostgresTopic,
                    "src/eth/storage/postgres/queries/select_topics_by_block_number.sql",
                    block_number
                )
                .fetch_all(&self.connection_pool);

                // run queries concurrently, but not in parallel
                // see https://docs.rs/tokio/latest/tokio/macro.join.html#runtime-characteristics
                let res = tokio::join!(header_query, transactions_query, logs_query, topics_query);
                let header = match res.0 {
                    Ok(Some(header)) => header,
                    Ok(None) => return Ok(None),
                    Err(e) => return log_and_err!(reason = e, "failed to query block by number"),
                };
                let transactions = res.1?;
                let logs = res.2?.into_iter();
                let topics = res.3?.into_iter();

                // We're still cloning the hashes, maybe create a HashMap structure like this
                // `HashMap<PostgresTransaction, Vec<HashMap<PostgresLog, Vec<PostgresTopic>>>>` in the future
                // so that we don't have to clone the hashes
                let mut log_partitions = partition_logs(logs);
                let mut topic_partitions = partition_topics(topics);
                let transactions = transactions
                    .into_iter()
                    .map(|tx| {
                        let this_tx_logs = log_partitions.remove(&tx.hash).unwrap_or_default();
                        let this_tx_topics = topic_partitions.remove(&tx.hash).unwrap_or_default();
                        tx.into_transaction_mined(this_tx_logs, this_tx_topics)
                    })
                    .collect();

                let block = Block { header, transactions };

                Ok::<Option<Block>, anyhow::Error>(Some(block))
            }
            BlockSelection::Earliest => {
                let block_number = 0i64;

                let header_query = sqlx::query_file_as!(BlockHeader, "src/eth/storage/postgres/queries/select_block_header_by_number.sql", block_number,)
                    .fetch_optional(&self.connection_pool);

                let transactions_query = sqlx::query_file_as!(
                    PostgresTransaction,
                    "src/eth/storage/postgres/queries/select_transactions_by_block_number.sql",
                    block_number
                )
                .fetch_all(&self.connection_pool);

                let logs_query = sqlx::query_file_as!(PostgresLog, "src/eth/storage/postgres/queries/select_logs_by_block_number.sql", block_number)
                    .fetch_all(&self.connection_pool);

                let topics_query = sqlx::query_file_as!(
                    PostgresTopic,
                    "src/eth/storage/postgres/queries/select_topics_by_block_number.sql",
                    block_number
                )
                .fetch_all(&self.connection_pool);

                // run queries concurrently, but not in parallel
                // see https://docs.rs/tokio/latest/tokio/macro.join.html#runtime-characteristics
                let res = tokio::join!(header_query, transactions_query, logs_query, topics_query);
                let header = match res.0 {
                    Ok(Some(header)) => header,
                    Ok(None) => return Ok(None),
                    Err(e) => return log_and_err!(reason = e, "failed to query block by earlist"),
                };
                let transactions = res.1?;
                let logs = res.2?.into_iter();
                let topics = res.3?.into_iter();

                // We're still cloning the hashes, maybe create a HashMap structure like this
                // `HashMap<PostgresTransaction, Vec<HashMap<PostgresLog, Vec<PostgresTopic>>>>` in the future
                // so that we don't have to clone the hashes
                let mut log_partitions = partition_logs(logs);
                let mut topic_partitions = partition_topics(topics);
                let transactions = transactions
                    .into_iter()
                    .map(|tx| {
                        let this_tx_logs = log_partitions.remove(&tx.hash).unwrap();
                        let this_tx_topics = topic_partitions.remove(&tx.hash).unwrap();
                        tx.into_transaction_mined(this_tx_logs, this_tx_topics)
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
            "src/eth/storage/postgres/queries/select_transaction_by_hash.sql",
            hash.as_ref()
        )
        .fetch_one(&self.connection_pool)
        .await;

        let transaction: PostgresTransaction = match transaction_query {
            Ok(res) => res,
            Err(sqlx::Error::RowNotFound) => return Ok(None),
            err => err?,
        };

        let logs = sqlx::query_file_as!(
            PostgresLog,
            "src/eth/storage/postgres/queries/select_logs_by_transaction_hash.sql",
            hash.as_ref()
        )
        .fetch_all(&self.connection_pool)
        .await?;

        let topics = sqlx::query_file_as!(
            PostgresTopic,
            "src/eth/storage/postgres/queries/select_topics_by_transaction_hash.sql",
            hash.as_ref()
        )
        .fetch_all(&self.connection_pool)
        .await?;

        let mut topic_partitions = partition_topics(topics);

        Ok(Some(
            transaction.into_transaction_mined(logs, topic_partitions.remove(hash).unwrap_or_default()),
        ))
    }

    async fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        tracing::debug!(filter = ?filter, "Reading logs");
        let from: i64 = filter.from_block.try_into()?;
        let query = include_str!("queries/select_logs.sql");

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

        let query_result = log_query.fetch_all(&self.connection_pool).await?;

        let mut result = vec![];

        for row in query_result {
            let block_hash: &[u8] = row.get("block_hash");
            let log_idx: i32 = row.get("log_idx");
            let topics = sqlx::query_file_as!(
                PostgresTopic,
                "src/eth/storage/postgres/queries/select_topics_by_block_hash_log_idx.sql",
                block_hash,
                log_idx
            )
            .fetch_all(&self.connection_pool)
            .await?;

            let log = LogMined {
                log: Log {
                    address: row.get("address"),
                    data: row.get("data"),
                    topics: topics.into_iter().map(LogTopic::from).collect(),
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
        let mut topic_batch = TopicBatch::default();
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
                for mut log in logs {
                    let topics = std::mem::take(&mut log.log.topics);
                    let tx_hash = log.transaction_hash.clone();
                    let log_index = log.log_index;
                    let tx_index = log.transaction_index;
                    let b_number = log.block_number;
                    let b_hash = log.block_hash.clone();

                    log_batch.push(log);
                    for (idx, topic) in topics.into_iter().enumerate() {
                        topic_batch.push(topic, idx, tx_hash.clone(), tx_index, log_index, b_number, b_hash.clone())?;
                    }
                }
            }
        }

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

            account_batch.push(
                change.address.clone(),
                new_nonce.clone().unwrap_or(original_nonce.clone()),
                new_balance.clone().unwrap_or(original_balance.clone()),
                bytecode,
                block.header.number,
                original_nonce,
                original_balance,
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
                historical_slot_batch.push(change.address.clone(), slot_idx, new_value, block.header.number);
            }
        }

        let expected_modified_slots = slot_batch.address.len();
        let expected_modified_accounts = account_batch.address.len();

        let mut tx = self.connection_pool.begin().await.context("failed to init save_block transaction")?;

        let block_result = sqlx::query_file!(
            "src/eth/storage/postgres/queries/insert_entire_block.sql",
            i64::try_from(block.header.number).context("failed to convert block number")?,
            block.header.hash.as_ref(),
            block.header.transactions_root.as_ref(),
            BigDecimal::try_from(block.header.gas.clone())?,
            block.header.bloom.as_ref(),
            i64::try_from(block.header.timestamp).context("failed to convert block timestamp")?,
            block.header.parent_hash.as_ref(),
            transaction_batch.hash as _,
            transaction_batch.signer as _,
            transaction_batch.nonce as _,
            transaction_batch.from as _,
            transaction_batch.to as _,
            transaction_batch.input as _,
            transaction_batch.output as _,
            transaction_batch.gas as _,
            transaction_batch.gas_price as _,
            transaction_batch.index as _,
            transaction_batch.block_number as _,
            transaction_batch.block_hash as _,
            transaction_batch.v as _,
            transaction_batch.r as _,
            transaction_batch.s as _,
            transaction_batch.value as _,
            &transaction_batch.result,
            log_batch.address as _,
            log_batch.data as _,
            log_batch.transaction_hash as _,
            log_batch.transaction_index as _,
            log_batch.log_index as _,
            log_batch.block_number as _,
            log_batch.block_hash as _,
            topic_batch.topic as _,
            topic_batch.transaction_hash as _,
            topic_batch.transaction_index as _,
            topic_batch.log_index as _,
            topic_batch.index as _,
            topic_batch.block_number as _,
            topic_batch.block_hash as _,
            account_batch.address as _,
            account_batch.bytecode as _,
            account_batch.new_balance as _,
            account_batch.new_nonce as _,
            account_batch.block_number as _,
            account_batch.original_balance as _,
            account_batch.original_nonce as _,
            slot_batch.index as _,
            slot_batch.value as _,
            slot_batch.address as _,
            slot_batch.block_number as _,
            slot_batch.original_value as _,
            historical_nonce_batch.address as _,
            historical_nonce_batch.nonce as _,
            historical_nonce_batch.block_number as _,
            historical_balance_batch.address as _,
            historical_balance_batch.balance as _,
            historical_balance_batch.block_number as _,
            historical_slot_batch.index as _,
            historical_slot_batch.value as _,
            historical_slot_batch.address as _,
            historical_slot_batch.block_number as _
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

    async fn read_current_block_number(&self) -> anyhow::Result<BlockNumber> {
        tracing::debug!("reading current block number");

        let currval: i64 = sqlx::query_file_scalar!("src/eth/storage/postgres/queries/select_current_block_number.sql")
            .fetch_one(&self.connection_pool)
            .await
            .unwrap_or(0);

        let block_number = BlockNumber::from(currval);
        Ok(block_number)
    }

    async fn save_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<()> {
        tracing::debug!(?accounts, "saving initial accounts");

        for acc in accounts {
            let mut tx = self.connection_pool.begin().await.context("failed to init transaction")?;
            let block_number = 0;
            let balance = BigDecimal::try_from(acc.balance)?;
            let nonce = BigDecimal::try_from(acc.nonce)?;
            let bytecode = acc.bytecode.as_deref();

            sqlx::query_file!(
                "src/eth/storage/postgres/queries/insert_account.sql",
                acc.address.as_ref(),
                nonce,
                balance,
                bytecode,
                block_number,
                BigDecimal::from(0),
                BigDecimal::from(0)
            )
            .execute(&mut *tx)
            .await
            .context("failed to insert account")?;

            sqlx::query_file!(
                "src/eth/storage/postgres/queries/insert_historical_balance.sql",
                acc.address.as_ref(),
                balance,
                block_number
            )
            .execute(&mut *tx)
            .await
            .context("failed to insert balance")?;

            sqlx::query_file!(
                "src/eth/storage/postgres/queries/insert_historical_nonce.sql",
                acc.address.as_ref(),
                nonce,
                block_number
            )
            .execute(&mut *tx)
            .await
            .context("failed to insert nonce")?;

            tx.commit().await.context("failed to commit transaction")?;
        }

        Ok(())
    }

    async fn reset_at(&self, number: BlockNumber) -> anyhow::Result<()> {
        sqlx::query!("DELETE FROM blocks WHERE number > $1", i64::try_from(number)?)
            .execute(&self.connection_pool)
            .await?;

        // Rollback the values of account.latest_balance, account.latest_nonce and
        // account_slots.value.

        sqlx::query_file!("src/eth/storage/postgres/queries/update_account_reset_balance.sql")
            .execute(&self.connection_pool)
            .await?;

        sqlx::query_file!("src/eth/storage/postgres/queries/update_account_reset_nonce.sql")
            .execute(&self.connection_pool)
            .await?;

        sqlx::query_file!("src/eth/storage/postgres/queries/update_account_slots_reset_value.sql")
            .execute(&self.connection_pool)
            .await?;

        Ok(())
    }

    async fn get_slots_sample(&self, start: BlockNumber, end: BlockNumber, max_samples: u64, seed: Option<u64>) -> anyhow::Result<Vec<SlotSample>> {
        let seed = (seed.unwrap_or(0) as f64 / 100.0).fract();
        let slots_sample_rows = sqlx::query_file_as!(
            SlotSample,
            "src/eth/storage/postgres/queries/select_random_slot_sample.sql",
            seed,
            start as _,
            end as _,
            max_samples as i64
        )
        .fetch_all(&self.connection_pool)
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

fn partition_topics(topics: impl IntoIterator<Item = PostgresTopic>) -> HashMap<TransactionHash, HashMap<LogIndex, Vec<PostgresTopic>>> {
    let mut partitions: HashMap<TransactionHash, HashMap<LogIndex, Vec<PostgresTopic>>> = HashMap::new();
    for topic in topics {
        match partitions.get_mut(&topic.transaction_hash) {
            Some(transaction_logs) => {
                if let Some(part) = transaction_logs.get_mut(&topic.log_idx) {
                    part.push(topic);
                } else {
                    transaction_logs.insert(topic.log_idx, vec![topic]);
                }
            }
            None => {
                partitions.insert(topic.transaction_hash.clone(), [(topic.log_idx, vec![topic])].into_iter().collect());
            }
        }
    }
    partitions
}
