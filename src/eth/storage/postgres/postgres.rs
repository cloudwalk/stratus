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
use crate::eth::primitives::Hash;
use crate::eth::primitives::Index;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionExecutionConflicts;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::postgres::types::PostgresLog;
use crate::eth::storage::postgres::types::PostgresTopic;
use crate::eth::storage::postgres::types::PostgresTransaction;
use crate::eth::storage::EthStorage;
use crate::eth::storage::EthStorageError;
use crate::infra::postgres::Postgres;

#[async_trait]
impl EthStorage for Postgres {
    async fn check_conflicts(&self, _execution: &TransactionExecution) -> anyhow::Result<TransactionExecutionConflicts> {
        todo!()
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
        .fetch_one(&self.connection_pool)
        .await
        .unwrap_or(Account {
            address: address.clone(),
            ..Account::default()
        });

        tracing::debug!(%address, "Account read");

        Ok(account)
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
        .fetch_one(&self.connection_pool)
        .await?;

        Ok(slot)
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
                        let this_tx_logs = log_partitions.remove(&tx.hash).unwrap();
                        let this_tx_topics = topic_partitions.remove(&tx.hash).unwrap();
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
                        let this_tx_logs = log_partitions.remove(&tx.hash).unwrap();
                        let this_tx_topics = topic_partitions.remove(&tx.hash).unwrap();
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
                        let this_tx_logs = log_partitions.remove(&tx.hash).unwrap();
                        let this_tx_topics = topic_partitions.remove(&tx.hash).unwrap();
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
        todo!()
    }

    async fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        let from: i64 = filter.from_block.try_into().unwrap();
        let to: i64;
        let query = r#"
            SELECT
                address as "address:  "
                , data as "data:  "
                , transaction_hash as "transaction_hash: "
                , log_idx as "log_idx:  "
                , block_number as "block_number:  "
                , block_hash as "block_hash:  "
            FROM logs
            WHERE block_number >= $1
        "#
        .to_owned();
        let builder = &mut QueryBuilder::new(query);
        builder.push_bind(from);

        // verifies if to_block exists
        if let Some(block_number) = filter.to_block {
            builder.push(" AND block_number <= $2");
            to = block_number.try_into().unwrap();
            builder.push_bind(to.to_owned());
        }

        let query = builder.build();
        let result = query
            .map(|row: PgRow| LogMined {
                log: crate::eth::primitives::Log {
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

    // The types conversions are ugly, but they are acting as a placeholder until we decide if we'll use
    // byte arrays for numbers. Implementing the trait sqlx::Encode for the eth primitives would make
    // this much easier to work with (note: I tried implementing Encode for both Hash and Nonce and
    //either worked for some reason I was not able to determine at this time)
    async fn save_block(&self, block: Block) -> anyhow::Result<(), EthStorageError> {
        tracing::debug!(block = ?block, "saving block");
        sqlx::query!(
            "INSERT INTO blocks(hash, transactions_root, gas, logs_bloom, timestamp_in_secs, created_at)
            VALUES ($1, $2, $3, $4, $5, current_timestamp)",
            block.header.hash.as_ref(),
            block.header.transactions_root.as_ref(),
            BigDecimal::from(block.header.gas),
            block.header.bloom.as_ref(),
            i32::try_from(block.header.timestamp_in_secs).context("failed to convert block timestamp")?
        )
        .execute(&self.connection_pool)
        .await
        .context("failed to insert block")?;

        for transaction in block.transactions {
            if transaction.is_success() {
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
                    sqlx::query!(
                        r#"
                INSERT INTO accounts
                VALUES ($1, $2, $3, $4, $5)
                ON CONFLICT (address) DO UPDATE
                SET nonce = EXCLUDED.nonce,
                    balance = EXCLUDED.balance,
                    bytecode = EXCLUDED.bytecode"#,
                        change.address.as_ref(),
                        BigDecimal::from(nonce),
                        BigDecimal::from(balance),
                        bytecode,
                        i64::try_from(block.header.number).context("failed to convert block number")?
                    )
                    .execute(&self.connection_pool)
                    .await
                    .context("failed to insert topic")?;
                }
            }

            sqlx::query!(
                "INSERT INTO transactions
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)",
                transaction.input.hash.as_ref(),
                transaction.input.signer.as_ref(),
                BigDecimal::from(transaction.input.nonce),
                transaction.input.signer.as_ref(),
                transaction.input.to.unwrap_or_default().as_ref().to_owned(),
                *transaction.input.input,
                *transaction.execution.output,
                BigDecimal::from(transaction.execution.gas),
                BigDecimal::from(transaction.input.gas_price),
                i32::from(transaction.transaction_index),
                i64::try_from(transaction.block_number).context("failed to convert block number")?,
                transaction.block_hash.as_ref(),
                &<[u8; 8]>::from(transaction.input.v),
                &<[u8; 32]>::from(transaction.input.r),
                &<[u8; 32]>::from(transaction.input.s),
                BigDecimal::from(transaction.input.value),
                transaction.execution.result.to_string()
            )
            .execute(&self.connection_pool)
            .await
            .context("failed to insert transaction")?;
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
