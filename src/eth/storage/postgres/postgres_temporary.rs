use std::time::Duration;

use async_trait::async_trait;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use super::types::TemporaryAccountBatch;
use super::types::TemporarySlotBatch;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::storage::TemporaryStorage;
use crate::log_and_err;

#[derive(Debug)]
pub struct PostgresTemporaryStorageConfig {
    pub url: String,
    pub connections: u32,
    pub acquire_timeout: Duration,
}

pub struct PostgresTemporaryStorage {
    pub pool: PgPool,
}

impl PostgresTemporaryStorage {
    pub async fn new(config: PostgresTemporaryStorageConfig) -> anyhow::Result<Self> {
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
impl TemporaryStorage for PostgresTemporaryStorage {
    async fn set_active_block_number(&self, number: BlockNumber) -> anyhow::Result<()> {
        sqlx::query!("UPDATE active_block_number SET block_number = $1", number as _)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn read_active_block_number(&self) -> anyhow::Result<Option<BlockNumber>> {
        sqlx::query_scalar!(r#"SELECT block_number as "0:_" FROM active_block_number"#)
            .fetch_one(&self.pool)
            .await?
            .map(TryInto::try_into)
            .transpose()
    }

    async fn read_account(&self, address: &Address) -> anyhow::Result<Option<Account>> {
        Ok(sqlx::query_as!(
            Account,
            r#"
                SELECT
                    address as "address: _",
                    latest_nonce as "nonce: _",
                    latest_balance as "balance: _",
                    bytecode as "bytecode: _",
                    code_hash as "code_hash: _",
                    static_slot_indexes as "static_slot_indexes: _",
                    mapping_slot_indexes as "mapping_slot_indexes: _"
                FROM accounts_temporary
                WHERE address = $1"#,
            address as _
        )
        .fetch_optional(&self.pool)
        .await?)
    }

    async fn read_slot(&self, address: &Address, index: &SlotIndex) -> anyhow::Result<Option<Slot>> {
        Ok(sqlx::query_scalar!(
            r#"
            SELECT value
                FROM account_slots_temporary
                WHERE account_address = $1
                AND idx = $2
            "#,
            address as _,
            index as _
        )
        .fetch_optional(&self.pool)
        .await?
        .map(|val| Slot {
            index: *index,
            value: val.into(),
        }))
    }
    // TODO: SLOTS
    async fn save_account_changes(&self, changes: Vec<ExecutionAccountChanges>) -> anyhow::Result<()> {
        let mut account_batch = TemporaryAccountBatch::default();
        let mut slots_batch = TemporarySlotBatch::default();

        for change in changes {
            for (idx, slot_change) in &change.slots {
                let (original, modified) = slot_change.clone().take_both();
                let new_value = match modified {
                    Some(s) => s.value,
                    None => {
                        tracing::trace!("slot value not set, skipping");
                        continue;
                    }
                };

                let original_value = original.unwrap_or_default().value;

                slots_batch.push(change.address, *idx, new_value, original_value);
            }
            account_batch.push_change(change)?;
        }
        sqlx::query!(
            r#"
            WITH slot_insert AS (
                WITH slot_updates AS (
                    SELECT *
                    FROM unnest(
                            $10::bytea [],
                            $11::bytea [],
                            $12::bytea [],
                            $13::bytea []
                        ) AS t (
                            idx,
                            value,
                            account_address,
                            original_value
                        )
                )
                INSERT INTO account_slots_temporary (
                        idx,
                        value,
                        previous_value,
                        account_address
                    )
                SELECT idx,
                    value,
                    original_value,
                    account_address
                FROM slot_updates ON CONFLICT (idx, account_address) DO
                UPDATE
                SET value = excluded.value,
                    previous_value = excluded.previous_value
                WHERE account_slots_temporary.value = excluded.previous_value
                RETURNING 1 as res
            ),
            account_updates AS (
                SELECT *
                FROM unnest(
                        $1::bytea [],
                        $2::bytea [],
                        $3::numeric [],
                        $4::numeric [],
                        $5::numeric [],
                        $6::numeric [],
                        $7::bytea [],
                        $8::JSONB [],
                        $9::JSONB []
                    ) AS t (
                        address,
                        bytecode,
                        new_balance,
                        new_nonce,
                        previous_balance,
                        previous_nonce,
                        code_hash,
                        static_slot_indexes,
                        mapping_slot_indexes
                    )
            )
            INSERT INTO accounts_temporary (
                    address,
                    bytecode,
                    latest_balance,
                    latest_nonce,
                    previous_balance,
                    previous_nonce,
                    code_hash,
                    static_slot_indexes,
                    mapping_slot_indexes
                )
            SELECT address,
                bytecode,
                new_balance,
                new_nonce,
                previous_balance,
                previous_nonce,
                code_hash,
                static_slot_indexes,
                mapping_slot_indexes
            FROM account_updates ON CONFLICT (address) DO
            UPDATE
            SET latest_nonce = excluded.latest_nonce,
                latest_balance = excluded.latest_balance,
                previous_balance = excluded.previous_balance,
                previous_nonce = excluded.previous_nonce
            WHERE accounts_temporary.latest_nonce = excluded.previous_nonce
                AND accounts_temporary.latest_balance = excluded.previous_balance
        "#,
            account_batch.address as _,
            account_batch.bytecode as _,
            account_batch.new_balance as _,
            account_batch.new_nonce as _,
            account_batch.original_balance as _,
            account_batch.original_nonce as _,
            account_batch.code_hash as _,
            account_batch.static_slot_indexes as _,
            account_batch.mapping_slot_indexes as _,
            slots_batch.index as _,
            slots_batch.value as _,
            slots_batch.original_value as _,
            slots_batch.address as _
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    // NOTE: Flushing would make sense when using postgres as both permanent and temporary since the flush could be done with one small query
    async fn flush(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn reset(&self) -> anyhow::Result<()> {
        let truncate_future = sqlx::query!(r#"TRUNCATE TABLE accounts_temporary, account_slots_temporary"#).execute(&self.pool);
        let block_number_future = sqlx::query!(r#"UPDATE active_block_number SET block_number = NULL"#).execute(&self.pool);
        tokio::try_join!(truncate_future, block_number_future)?;
        Ok(())
    }
}
