use async_trait::async_trait;

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
use crate::eth::storage::postgres::types::PostgresLog;
use crate::eth::storage::postgres::types::PostgresTopic;
use crate::eth::storage::postgres::types::PostgresTransaction;
use crate::eth::storage::EthStorage;
use crate::infra::postgres::Postgres;

#[async_trait]
impl EthStorage for Postgres {
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
        .await?;

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
                            ,block_timestamp as "block_timestamp: _"
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

                todo!()
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
            }
            BlockSelection::Earliest => {
                todo!()
            }
        };

        todo!();
    }

    async fn read_mined_transaction(&self, hash: &Hash) -> anyhow::Result<Option<TransactionMined>> {
        tracing::debug!(%hash, "reading transaction");
        todo!()
    }

    async fn read_logs(&self, _: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        Ok(Vec::new())
    }

    async fn save_block(&self, block: Block) -> anyhow::Result<()> {
        tracing::debug!(block = ?block, "saving block");
        todo!()
    }

    async fn read_current_block_number(&self) -> anyhow::Result<BlockNumber> {
        tracing::debug!("reading current block number");

        let currval: i64 = sqlx::query_scalar!(
            r#"
                        SELECT CURRVAL('block_number_seq') as "n!: _"
                    "#
        )
        .fetch_one(&self.connection_pool)
        .await?;

        let block_number = BlockNumber::from(currval);

        Ok(block_number)
    }

    async fn increment_block_number(&self) -> anyhow::Result<BlockNumber> {
        tracing::debug!("incrementing block number");

        let nextval: i64 = sqlx::query_scalar!(
            r#"
                        SELECT NEXTVAL('block_number_seq') as "n!: _"
                    "#
        )
        .fetch_one(&self.connection_pool)
        .await?;

        let block_number = BlockNumber::from(nextval);

        Ok(block_number)
    }
}
