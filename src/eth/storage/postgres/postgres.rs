use std::collections::HashMap;

use anyhow::Context;
use async_trait::async_trait;
use sqlx::types::BigDecimal;

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

        // This query is overflowing on the second transaction when the evm tries to read the account
        // reading address=0x00000000000000000000000000000000000000ff
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
                .fetch_one(&self.connection_pool)
                .await?;

                todo!()
            }
            BlockSelection::Hash(hash) => {
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
                            ,gas_price as "gas_price: _"
                            ,idx_in_block as "idx_in_block: _"
                            ,block_number as "block_number: _"
                            ,block_hash as "block_hash: _"
                            ,output as "output: _"
                            ,value as "value: _"
                            ,v as "v: _"
                            ,s as "s: _"
                            ,r as "r: _"
                            ,result as "result: _"
                        FROM transactions
                        WHERE hash = $1
                    "#,
                    hash.as_ref()
                )
                .fetch_all(&self.connection_pool);

                let logs_query = sqlx::query_as!(
                    PostgresLog,
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

                let _ = sqlx::query_as!(
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
                .await;
                todo!()
            }
            BlockSelection::Earliest => {
                todo!()
            }
        }
    }

    async fn read_mined_transaction(&self, hash: &Hash) -> anyhow::Result<Option<TransactionMined>> {
        tracing::debug!(%hash, "reading transaction");
        todo!()
    }

    async fn read_logs(&self, _: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        Ok(Vec::new())
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
            for change in transaction.execution.changes {
                let nonce = change.nonce.take().unwrap_or(0.into());
                let balance = change.balance.take().unwrap_or(0.into());
                let bytecode = change.bytecode.take().unwrap_or(None).map(|val| val.as_ref().to_owned());
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
                .context("failed to insert account")?;
            }

            for log in transaction.logs {
                let tx_hash = log.transaction_hash.as_ref();
                let tx_idx = i32::from(log.transaction_index);
                let lg_idx = i32::from(log.log_index);
                let b_number = i64::try_from(log.block_number).context("failed to convert block number")?;
                let b_hash = log.block_hash.as_ref();
                sqlx::query!(
                    "INSERT INTO logs VALUES ($1, $2, $3, $4, $5, $6, $7)",
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
                    sqlx::query!(
                        "INSERT INTO topics VALUES ($1, $2, $3, $4, $5, $6)",
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
