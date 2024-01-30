use std::collections::HashMap;

use anyhow::Context;
use async_trait::async_trait;
use sqlx::postgres::PgRow;
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
use crate::eth::primitives::Index;
use crate::eth::primitives::Log;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
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
    async fn check_conflicts(&self, _execution: &Execution) -> anyhow::Result<ExecutionConflicts> {
        // TODO: implement conflict resolution
        Ok(ExecutionConflicts::default())
    }

    async fn read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Account> {
        tracing::debug!(%address, "reading account");

        // TODO: use HistoricalValue
        let block = match point_in_time {
            StoragePointInTime::Present => self.read_current_block_number().await?,
            StoragePointInTime::Past(number) => *number,
        };

        let block_number = i64::try_from(block)?;
        let account = sqlx::query_as!(
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
        .fetch_optional(&self.connection_pool)
        .await?;

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

        // TODO: use HistoricalValue
        let block = match point_in_time {
            StoragePointInTime::Present => self.read_current_block_number().await?,
            StoragePointInTime::Past(number) => *number,
        };

        let block_number = i64::try_from(block)?;

        // TODO: improve this conversion
        let slot_index: [u8; 32] = slot_index.clone().into();

        let slot = sqlx::query_as!(
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
        .fetch_optional(&self.connection_pool)
        .await?;

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
                        let this_tx_logs = log_partitions.remove(&tx.hash).unwrap_or(Vec::new());
                        let this_tx_topics = topic_partitions.remove(&tx.hash).unwrap_or(HashMap::new());
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
        let transaction = sqlx::query_file_as!(
            PostgresTransaction,
            "src/eth/storage/postgres/queries/select_transaction_by_hash.sql",
            hash.as_ref()
        )
        .fetch_one(&self.connection_pool)
        .await
        .map_err(|err| {
            tracing::debug!(?err);
            err
        })?;

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
            transaction.into_transaction_mined(logs, topic_partitions.remove(hash).unwrap_or(HashMap::new())),
        ))
    }

    async fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        let from: i64 = filter.from_block.try_into()?;
        let query = include_str!("queries/select_logs.sql");

        let builder = &mut QueryBuilder::new(query);
        builder.push("AND block_number >= $1");
        builder.push_bind(from);

        // verifies if to_block exists
        if let Some(block_number) = filter.to_block {
            builder.push(" AND block_number <= $2");
            let to: i64 = block_number.try_into()?;
            builder.push_bind(to);
        }

        let query = builder.build();
        let result = query
            .map(|row: PgRow| LogMined {
                log: Log {
                    address: row.get("address"),
                    data: row.get("data"),
                    topics: vec![],
                },
                transaction_hash: row.get("transaction_hash"),
                transaction_index: row.get("transaction_idx"),
                log_index: row.get("log_idx"),
                block_number: row.get("block_number"),
                block_hash: row.get("block_hash"),
            })
            .fetch_all(&self.connection_pool)
            .await?
            .into_iter()
            .filter(|log| filter.matches(log))
            .collect();

        Ok(result)
    }

    // The type conversions are ugly, but they are acting as a placeholder until we decide if we'll use
    // byte arrays for numbers. Implementing the trait sqlx::Encode for the eth primitives would make
    // this much easier to work with (note: I tried implementing Encode for both Hash and Nonce and
    // neither worked for some reason I was not able to determine at this time)
    // TODO: save slots
    async fn save_block(&self, block: Block) -> anyhow::Result<(), EthStorageError> {
        tracing::debug!(block = ?block, "saving block");
        sqlx::query_file!(
            "src/eth/storage/postgres/queries/insert_block.sql",
            i64::try_from(block.header.number).context("failed to convert block number")?,
            block.header.hash.as_ref(),
            block.header.transactions_root.as_ref(),
            BigDecimal::try_from(block.header.gas)?,
            block.header.bloom.as_ref(),
            i32::try_from(block.header.timestamp_in_secs).context("failed to convert block timestamp")?
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

                    sqlx::query_file!(
                        "src/eth/storage/postgres/queries/insert_account.sql",
                        change.address.as_ref(),
                        BigDecimal::try_from(nonce)?,
                        BigDecimal::try_from(balance)?,
                        bytecode,
                        i64::try_from(block.header.number).context("failed to convert block number")?
                    )
                    .execute(&self.connection_pool)
                    .await
                    .context("failed to insert account")?;
                    for (slot_idx, value) in change.slots {
                        sqlx::query_file!(
                            "src/eth/storage/postgres/queries/insert_account_slot.sql",
                            &<[u8; 32]>::from(slot_idx),
                            &<[u8; 32]>::from(value.take().ok_or(anyhow::anyhow!("critical: no change for slot"))?.value), // this should never happen
                            change.address.as_ref(),
                            i64::try_from(block.header.number).context("failed to convert block number")?
                        )
                        .execute(&self.connection_pool)
                        .await
                        .context("failed to insert slot")?;
                    }
                }
            }

            for log in transaction.logs {
                let tx_hash = log.transaction_hash.as_ref();
                let tx_idx = i32::from(log.transaction_index);
                let lg_idx = i32::from(log.log_index);
                let b_number = i64::try_from(log.block_number).context("failed to convert block number")?;
                let b_hash = log.block_hash.as_ref();
                sqlx::query_file!(
                    "src/eth/storage/postgres/queries/insert_log.sql",
                    log.log.address.as_ref(),
                    *log.log.data,
                    tx_hash,
                    tx_idx,
                    lg_idx,
                    b_number,
                    b_hash
                )
                .execute(&self.connection_pool)
                .await
                .context("failed to insert log")?;
                for topic in log.log.topics {
                    sqlx::query_file!(
                        "src/eth/storage/postgres/queries/insert_topic.sql",
                        topic.as_ref(),
                        tx_hash,
                        tx_idx,
                        lg_idx,
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
        .unwrap_or(0)
            + 1;

        let block_number = BlockNumber::from(nextval);

        Ok(block_number)
    }
}

fn partition_logs(logs: impl IntoIterator<Item = PostgresLog>) -> HashMap<Hash, Vec<PostgresLog>> {
    let mut partitions: HashMap<Hash, Vec<PostgresLog>> = HashMap::new();
    for log in logs {
        if let Some(part) = partitions.get_mut(&log.transaction_hash) {
            part.push(log);
        } else {
            partitions.insert(log.transaction_hash.clone(), vec![log]);
        }
    }
    partitions
}

fn partition_topics(topics: impl IntoIterator<Item = PostgresTopic>) -> HashMap<Hash, HashMap<Index, Vec<PostgresTopic>>> {
    let mut partitions: HashMap<Hash, HashMap<Index, Vec<PostgresTopic>>> = HashMap::new();
    for topic in topics {
        if let Some(transaction) = partitions.get_mut(&topic.transaction_hash) {
            if let Some(part) = transaction.get_mut(&topic.log_idx) {
                part.push(topic);
            } else {
                transaction.insert(topic.log_idx, vec![topic]);
            }
        } else {
            partitions.insert(topic.transaction_hash.clone(), HashMap::new());
        }
    }
    partitions
}
