use std::collections::HashMap;

use anyhow::Context;
use async_trait::async_trait;
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
use crate::eth::storage::EthStorage;
use crate::eth::storage::EthStorageError;
use crate::infra::postgres::Postgres;

#[async_trait]
impl EthStorage for Postgres {
    async fn check_conflicts(&self, _execution: &Execution) -> anyhow::Result<Option<ExecutionConflicts>> {
        // TODO: implement conflict resolution
        Ok(None)
    }

    async fn read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Account> {
        tracing::debug!(%address, "reading account");
        let account = match point_in_time {
            StoragePointInTime::Present => {
                // We have to get the account information closest to the block with the given block_number
                sqlx::query_as!(
                    Account,
                    r#"
                    SELECT
                        address as "address: _",
                        latest_nonce as "nonce: _",
                        latest_balance as "balance: _",
                        bytecode as "bytecode: _"
                    FROM accounts
                    WHERE address = $1
                    "#,
                    address.as_ref()
                )
                .fetch_optional(&self.connection_pool)
                .await?
            }
            StoragePointInTime::Past(number) => {
                let block_number: i64 = (*number).try_into()?;
                // We have to get the account information closest to the block with the given block_number
                sqlx::query_as!(
                    Account,
                    r#"
                    SELECT
                        accounts.address as "address: _",
                        historical_nonces.nonce as "nonce: _",
                        historical_balances.balance as "balance: _",
                        bytecode as "bytecode: _"
                    FROM accounts
                    JOIN historical_nonces
                    ON accounts.address = historical_nonces.address
                    JOIN historical_balances
                    ON accounts.address = historical_balances.address
                    WHERE accounts.address = $1
                    AND historical_nonces.block_number = (SELECT MAX(block_number)
                                                            FROM historical_nonces
                                                            WHERE block_number <= $2
                                                            AND address = $1)
                    AND historical_balances.block_number = (SELECT MAX(block_number)
                                                            FROM historical_balances
                                                            WHERE block_number <= $2
                                                            AND address = $1)
                            "#,
                    address.as_ref(),
                    block_number,
                )
                .fetch_optional(&self.connection_pool)
                .await?
            }
        };

        // If there is no account, we return
        // an "empty account"
        let acc = match account {
            Some(acc) => acc,
            None => Account {
                address: address.clone(),
                ..Account::default()
            },
        };

        tracing::debug!(%address, "Account read");

        Ok(acc)
    }

    async fn read_slot(&self, address: &Address, slot_index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Slot> {
        tracing::debug!(%address, %slot_index, "reading slot");

        // TODO: improve this conversion
        let slot_index: [u8; 32] = slot_index.clone().into();

        let slot = match point_in_time {
            StoragePointInTime::Present => {
                sqlx::query_as!(
                    Slot,
                    r#"
                        SELECT
                            idx as "index: _",
                            value as "value: _"
                        FROM account_slots
                        WHERE account_address = $1
                        AND idx = $2
                    "#,
                    address.as_ref(),
                    slot_index.as_ref()
                )
                .fetch_optional(&self.connection_pool)
                .await?
            }
            StoragePointInTime::Past(number) => {
                let block_number: i64 = (*number).try_into()?;
                sqlx::query_as!(
                    Slot,
                    r#"
                        SELECT
                            idx as "index: _",
                            value as "value: _"
                        FROM historical_slots
                        WHERE account_address = $1
                        AND idx = $2
                        AND block_number = (SELECT MAX(block_number)
                                                FROM historical_slots
                                                WHERE account_address = $1
                                                AND idx = $2
                                                AND block_number <= $3)
                    "#,
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
        let s = match slot {
            Some(slot) => slot,
            None => Slot::default(),
        };

        Ok(s)
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
    async fn save_block(&self, block: Block) -> anyhow::Result<(), EthStorageError> {
        tracing::debug!(block = ?block, "saving block");
        sqlx::query_file!(
            "src/eth/storage/postgres/queries/insert_block.sql",
            i64::try_from(block.header.number).context("failed to convert block number")?,
            block.header.hash.as_ref(),
            block.header.transactions_root.as_ref(),
            BigDecimal::try_from(block.header.gas)?,
            block.header.bloom.as_ref(),
            i32::try_from(block.header.timestamp_in_secs).context("failed to convert block timestamp")?,
            block.header.parent_hash.as_ref()
        )
        .execute(&self.connection_pool)
        .await
        .context("failed to insert block")?;

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
            .execute(&self.connection_pool)
            .await
            .context("failed to insert transaction")?;

            if is_success {
                for change in transaction.execution.changes {
                    let nonce = change.nonce.take().unwrap_or_else(|| {
                        tracing::debug!("Nonce not set, defaulting to 0");
                        0.into()
                    });
                    let balance = change.balance.take().unwrap_or_else(|| {
                        tracing::debug!("Balance not set, defaulting to 0");
                        0.into()
                    });
                    let bytecode = change
                        .bytecode
                        .take()
                        .unwrap_or_else(|| {
                            tracing::debug!("Bytecode not set, defaulting to None");
                            None
                        })
                        .map(|val| val.as_ref().to_owned());

                    let mut tx = self.connection_pool.begin().await.context("failed to init transaction")?;
                    let block_number = i64::try_from(block.header.number).context("failed to convert block number")?;
                    let balance = BigDecimal::try_from(balance)?;
                    let nonce = BigDecimal::try_from(nonce)?;

                    sqlx::query_file!(
                        "src/eth/storage/postgres/queries/insert_account.sql",
                        change.address.as_ref(),
                        nonce,
                        balance,
                        bytecode,
                        block_number
                    )
                    .execute(&mut *tx)
                    .await
                    .context("failed to insert account")?;

                    sqlx::query!(
                        "INSERT INTO historical_balances (address, balance, block_number) VALUES ($1, $2, $3)",
                        change.address.as_ref(),
                        balance,
                        block_number
                    )
                    .execute(&mut *tx)
                    .await
                    .context("failed to insert balance")?;

                    sqlx::query!(
                        "INSERT INTO historical_nonces (address, nonce, block_number) VALUES ($1, $2, $3)",
                        change.address.as_ref(),
                        nonce,
                        block_number
                    )
                    .execute(&mut *tx)
                    .await
                    .context("failed to insert nonce")?;

                    tx.commit().await.context("Failed to commit transaction")?;

                    for (slot_idx, value) in change.slots {
                        let mut tx = self.connection_pool.begin().await.context("failed to init transaction")?;
                        let idx = &<[u8; 32]>::from(slot_idx);
                        let val = &<[u8; 32]>::from(value.take().ok_or(anyhow::anyhow!("critical: no change for slot"))?.value); // the or condition should never happen
                        let block_number = i64::try_from(block.header.number).context("failed to convert block number")?;

                        sqlx::query_file!(
                            "src/eth/storage/postgres/queries/insert_account_slot.sql",
                            idx,
                            val,
                            change.address.as_ref(),
                            block_number
                        )
                        .execute(&mut *tx)
                        .await
                        .context("failed to insert slot")?;

                        sqlx::query_file!(
                            "src/eth/storage/postgres/queries/insert_historical_slot.sql",
                            idx,
                            val,
                            change.address.as_ref(),
                            block_number
                        )
                        .execute(&mut *tx)
                        .await
                        .context("failed to insert slot to history")?;

                        tx.commit().await.context("failed to commit transaction")?;
                    }
                }
            }

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
                .execute(&self.connection_pool)
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
                    .execute(&self.connection_pool)
                    .await
                    .context("failed to insert topic")?;
                }
            }
        }

        Ok(())
    }

    async fn read_current_block_number(&self) -> anyhow::Result<BlockNumber> {
        tracing::debug!("reading current block number");

        let currval: i64 = sqlx::query_scalar!(
            r#"
                SELECT MAX(number) as "n!: _" FROM blocks
                    "#
        )
        .fetch_one(&self.connection_pool)
        .await
        .unwrap_or(0);

        let block_number = BlockNumber::from(currval);
        Ok(block_number)
    }

    async fn increment_block_number(&self) -> anyhow::Result<BlockNumber> {
        tracing::debug!("incrementing block number");

        let nextval: i64 = sqlx::query_scalar!(
            r#"
                SELECT MAX(number) as "n!: _" FROM blocks
            "#
        )
        .fetch_one(&self.connection_pool)
        .await
        .unwrap_or_else(|err| {
            tracing::error!(?err, "failed to get block number");
            0
        }) + 1;

        Ok(nextval.into())
    }

    async fn reset(&self, number: BlockNumber) -> anyhow::Result<()> {
        sqlx::query!("DELETE FROM blocks WHERE number > $1", i64::try_from(number)?)
            .execute(&self.connection_pool)
            .await?;

        // Rollback the values of account.latest_balance, account.latest_nonce and
        // account_slots.value.

        sqlx::query!(
            r#"
            -- Get the block_numbers with the latest balance change for each address
            WITH latest_block_numbers AS (
                SELECT address, MAX(block_number) as block_number
                FROM historical_balances
                GROUP BY address
            ),
            -- Get the latest balance change for each address
            latest_balances AS (
                SELECT accounts.address, historical_balances.balance
                FROM accounts
                JOIN latest_block_numbers
                ON latest_block_numbers.address = accounts.address
                LEFT JOIN historical_balances on accounts.address = historical_balances.address
                    AND latest_block_numbers.block_number = historical_balances.block_number
            )
            -- Update the accounts with the balances in latest_balances
            UPDATE accounts
            SET latest_balance = latest_balances.balance
            FROM latest_balances
            WHERE latest_balances.address = accounts.address
                AND latest_balances.balance IS DISTINCT FROM accounts.latest_balance"#
        )
        .execute(&self.connection_pool)
        .await?;

        sqlx::query!(
            r#"
            WITH latest_block_numbers AS (
                SELECT address, MAX(block_number) as block_number
                FROM historical_nonces
                GROUP BY address
            ),
            latest_nonces AS (
                SELECT accounts.address, historical_nonces.nonce
                FROM accounts
                JOIN latest_block_numbers
                ON latest_block_numbers.address = accounts.address
                LEFT JOIN historical_nonces on accounts.address = historical_nonces.address
                    AND latest_block_numbers.block_number = historical_nonces.block_number
            )
            UPDATE accounts
            SET latest_nonce = latest_nonces.nonce
            FROM latest_nonces
            WHERE latest_nonces.address = accounts.address
                AND latest_nonces.nonce IS DISTINCT FROM accounts.latest_nonce"#
        )
        .execute(&self.connection_pool)
        .await?;

        sqlx::query!(
            r#"
            WITH latest_block_numbers AS (
                SELECT account_address, idx, MAX(block_number) as block_number
                FROM historical_slots
                GROUP BY idx, account_address
            ),
            latest_slots AS (
                SELECT account_slots.account_address, historical_slots.idx, historical_slots.value
                FROM account_slots
                JOIN latest_block_numbers
                ON latest_block_numbers.account_address = account_slots.account_address
                    AND account_slots.idx = latest_block_numbers.idx
                LEFT JOIN historical_slots ON account_slots.account_address = historical_slots.account_address
                    AND latest_block_numbers.block_number = historical_slots.block_number
              		AND account_slots.idx = historical_slots.idx
            )
            UPDATE account_slots
            SET value = latest_slots.value
            FROM latest_slots
            WHERE latest_slots.account_address = account_slots.account_address
                AND latest_slots.idx = account_slots.idx
                AND latest_slots.value IS DISTINCT FROM account_slots.value"#
        )
        .execute(&self.connection_pool)
        .await?;

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
