use std::collections::HashMap;

use anyhow::Context;
use async_trait::async_trait;
use nonempty::nonempty;
use sqlx::postgres::PgQueryResult;
use sqlx::query_builder::QueryBuilder;
use sqlx::types::BigDecimal;
use sqlx::Row;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockHeader;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
use crate::eth::primitives::Execution;
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
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::postgres::types::PostgresLog;
use crate::eth::storage::postgres::types::PostgresTopic;
use crate::eth::storage::postgres::types::PostgresTransaction;
use crate::eth::storage::PermanentStorage;
use crate::eth::storage::StorageError;
use crate::infra::postgres::Postgres;

#[async_trait]
impl PermanentStorage for Postgres {
    async fn check_conflicts(&self, _execution: &Execution) -> anyhow::Result<Option<ExecutionConflicts>> {
        Ok(None)
    }

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
            StoragePointInTime::Present =>
                sqlx::query_file_as!(Slot, "src/eth/storage/postgres/queries/select_slot.sql", address.as_ref(), slot_index.as_ref())
                    .fetch_optional(&self.connection_pool)
                    .await?,
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
                    .fetch_one(&self.connection_pool);

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
                let header = res.0?;
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
                    .fetch_one(&self.connection_pool);

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
                let header = res.0?;
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
                    .fetch_one(&self.connection_pool);

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
                let header = res.0?;
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
                    .fetch_one(&self.connection_pool);

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
                let header = res.0?;
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

    // The type conversions are ugly, but they are acting as a placeholder until we decide if we'll use
    // byte arrays for numbers. Implementing the trait sqlx::Encode for the eth primitives would make
    // this much easier to work with (note: I tried implementing Encode for both Hash and Nonce and
    // neither worked for some reason I was not able to determine at this time)
    async fn save_block(&self, block: Block) -> anyhow::Result<(), StorageError> {
        let mut tx = self.connection_pool.begin().await.context("failed to init save_block transaction")?;

        tracing::debug!(block = ?block, "saving block");
        sqlx::query_file!(
            "src/eth/storage/postgres/queries/insert_block.sql",
            i64::try_from(block.header.number).context("failed to convert block number")?,
            block.header.hash.as_ref(),
            block.header.transactions_root.as_ref(),
            BigDecimal::try_from(block.header.gas.clone())?,
            block.header.bloom.as_ref(),
            i64::try_from(block.header.timestamp).context("failed to convert block timestamp")?,
            block.header.parent_hash.as_ref()
        )
        .execute(&mut *tx)
        .await
        .context("failed to insert block")?;

        let account_changes = block.generate_execution_changes();

        for transaction in block.transactions {
            let is_success = transaction.is_success();
            let to = <[u8; 20]>::from(*transaction.input.to.unwrap_or_default());
            sqlx::query_file!(
                "src/eth/storage/postgres/queries/insert_transaction.sql",
                transaction.input.hash.as_ref(),
                transaction.input.signer.as_ref(),
                BigDecimal::try_from(transaction.input.nonce)?,
                transaction.input.signer.as_ref(),
                &to,
                *transaction.input.input,
                *transaction.execution.output,
                BigDecimal::try_from(transaction.execution.gas)?,
                BigDecimal::try_from(transaction.input.gas_price)?,
                i32::from(transaction.transaction_index),
                i64::try_from(transaction.block_number).context("failed to convert block number")?,
                transaction.block_hash.as_ref(),
                &<[u8; 8]>::from(transaction.input.v),
                &<[u8; 32]>::from(transaction.input.r),
                &<[u8; 32]>::from(transaction.input.s),
                BigDecimal::try_from(transaction.input.value)?,
                transaction.execution.result.to_string()
            )
            .execute(&mut *tx)
            .await
            .context("failed to insert transaction")?;

            if is_success {
                for log in transaction.logs {
                    let addr = log.log.address.as_ref();
                    let data = log.log.data;
                    let tx_hash = log.transaction_hash.as_ref();
                    let tx_idx = i32::from(log.transaction_index);
                    let lg_idx = i32::from(log.log_index);
                    let b_number = i64::try_from(log.block_number).context("failed to convert block number")?;
                    let b_hash = log.block_hash.as_ref();
                    sqlx::query_file!(
                        "src/eth/storage/postgres/queries/insert_log.sql",
                        addr,
                        *data,
                        tx_hash,
                        tx_idx,
                        lg_idx,
                        b_number,
                        b_hash
                    )
                    .execute(&mut *tx)
                    .await
                    .context("failed to insert log")?;
                    for (idx, topic) in log.log.topics.into_iter().enumerate() {
                        sqlx::query_file!(
                            "src/eth/storage/postgres/queries/insert_topic.sql",
                            topic.as_ref(),
                            tx_hash,
                            tx_idx,
                            lg_idx,
                            i32::try_from(idx).context("failed to convert topic idx")?,
                            b_number,
                            b_hash
                        )
                        .execute(&mut *tx)
                        .await
                        .context("failed to insert topic")?;
                    }
                }
            }
        }

        for change in account_changes {
            // for change in transaction.execution.changes {
            let (original_nonce, new_nonce) = change.nonce.take_both();
            let (original_balance, new_balance) = change.balance.take_both();

            let new_nonce: Option<BigDecimal> = match new_nonce {
                Some(nonce) => Some(nonce.try_into()?),
                None => None,
            };

            let new_balance: Option<BigDecimal> = match new_balance {
                Some(balance) => Some(balance.try_into()?),
                None => None,
            };

            let original_nonce: BigDecimal = original_nonce.unwrap_or_default().try_into()?;
            let original_balance: BigDecimal = original_balance.unwrap_or_default().try_into()?;

            let bytecode = change
                .bytecode
                .take()
                .unwrap_or_else(|| {
                    tracing::debug!("bytecode not set, defaulting to None");
                    None
                })
                .map(|val| val.as_ref().to_owned());

            let block_number = i64::try_from(block.header.number).context("failed to convert block number")?;

            let account_result: PgQueryResult = sqlx::query_file!(
                "src/eth/storage/postgres/queries/insert_account.sql",
                change.address.as_ref(),
                new_nonce.as_ref().unwrap_or(&original_nonce),
                new_balance.as_ref().unwrap_or(&original_balance),
                bytecode,
                block_number,
                original_nonce,
                original_balance
            )
            .execute(&mut *tx)
            .await
            .context("failed to insert account")?;

            // A successful insert/update with no conflicts will have one affected row
            if account_result.rows_affected() != 1 {
                tx.rollback().await.context("failed to rollback transaction")?;
                let error: StorageError = StorageError::Conflict(ExecutionConflicts(nonempty![ExecutionConflict::Account {
                    address: change.address,
                    expected_balance: original_balance,
                    expected_nonce: original_nonce,
                }]));
                return Err(error);
            }

            if let Some(balance) = new_balance {
                sqlx::query_file!(
                    "src/eth/storage/postgres/queries/insert_historical_balance.sql",
                    change.address.as_ref(),
                    balance,
                    block_number
                )
                .execute(&mut *tx)
                .await
                .context("failed to insert balance")?;
            }

            if let Some(nonce) = new_nonce {
                sqlx::query_file!(
                    "src/eth/storage/postgres/queries/insert_historical_nonce.sql",
                    change.address.as_ref(),
                    nonce,
                    block_number
                )
                .execute(&mut *tx)
                .await
                .context("failed to insert nonce")?;
            }

            for (slot_idx, value) in change.slots {
                let (original_value, val) = value.clone().take_both();
                let idx: [u8; 32] = slot_idx.into();
                let val: [u8; 32] = val.ok_or(anyhow::anyhow!("critical: no change for slot"))?.value.into(); // the or condition should never happen
                let block_number = i64::try_from(block.header.number).context("failed to convert block number")?;
                let original_value: [u8; 32] = original_value.unwrap_or_default().value.into();

                let slot_result: PgQueryResult = sqlx::query_file!(
                    "src/eth/storage/postgres/queries/insert_account_slot.sql",
                    &idx,
                    &val,
                    change.address.as_ref(),
                    block_number,
                    &original_value
                )
                .execute(&mut *tx)
                .await
                .context("failed to insert slot")?;

                // A successful insert/update with no conflicts will have one affected row
                if slot_result.rows_affected() != 1 {
                    tx.rollback().await.context("failed to rollback transaction")?;
                    let error: StorageError = StorageError::Conflict(ExecutionConflicts(nonempty![ExecutionConflict::PgSlot {
                        address: change.address,
                        slot: idx,
                        expected: original_value,
                    }]));
                    return Err(error);
                }

                sqlx::query_file!(
                    "src/eth/storage/postgres/queries/insert_historical_slot.sql",
                    &idx,
                    &val,
                    change.address.as_ref(),
                    block_number
                )
                .execute(&mut *tx)
                .await
                .context("failed to insert slot to history")?;
            }
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

    async fn enable_genesis(&self, genesis: Block) -> anyhow::Result<()> {
        let existing_genesis = sqlx::query_file!("src/eth/storage/postgres/queries/select_genesis.sql")
            .fetch_optional(&self.connection_pool)
            .await?;

        if existing_genesis.is_none() {
            self.save_block(genesis).await?;
        }

        Ok(())
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
            Some(transaction_logs) =>
                if let Some(part) = transaction_logs.get_mut(&topic.log_idx) {
                    part.push(topic);
                } else {
                    transaction_logs.insert(topic.log_idx, vec![topic]);
                },
            None => {
                partitions.insert(topic.transaction_hash.clone(), [(topic.log_idx, vec![topic])].into_iter().collect());
            }
        }
    }
    partitions
}
