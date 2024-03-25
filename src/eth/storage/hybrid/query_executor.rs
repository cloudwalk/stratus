use std::sync::Arc;

use sqlx::Acquire;
use sqlx::Pool;
use sqlx::Postgres;
use tokio::time::sleep;
use tokio::time::Duration;

use super::BlockTask;
use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::CodeHash;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotValue;
use crate::eth::primitives::Wei;

type BlockNumbers = Vec<i64>;
type Addresses = Vec<Address>;
type OptionalBytes = Vec<Option<Bytes>>;
type Weis = Vec<Wei>;
type Nonces = Vec<Nonce>;
type CodeHashs = Vec<CodeHash>;

type AccountChanges = (BlockNumbers, Addresses, OptionalBytes, Weis, Nonces, CodeHashs);
type AccountSlotChanges = (BlockNumbers, Vec<SlotIndex>, Addresses, Vec<SlotValue>);

async fn execute_with_retry<F, Fut>(mut attempt: F, max_attempts: u32, initial_delay: Duration) -> Result<(), sqlx::Error>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<(), sqlx::Error>>,
{
    let mut attempts = 0;
    let mut delay = initial_delay;

    loop {
        match attempt().await {
            Ok(_) => return Ok(()),
            Err(e) if attempts < max_attempts => {
                attempts += 1;
                tracing::warn!("Attempt {} failed, retrying in {:?}: {}", attempts, delay, e);
                sleep(delay).await;
                delay *= 2; // Exponential backoff
            }
            Err(e) => return Err(e),
        }
    }
}

pub async fn commit_eventually(pool: Arc<Pool<Postgres>>, block_task: BlockTask) {
    let block_data = serde_json::to_value(&block_task.block_data).unwrap();
    let account_changes = serde_json::to_value(&block_task.account_changes).unwrap();
    let mut accounts_changes: AccountChanges = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut accounts_slots_changes: AccountSlotChanges = (Vec::new(), Vec::new(), Vec::new(), Vec::new());

    for changes in block_task.account_changes.clone() {
        let (original_nonce, new_nonce) = changes.nonce.take_both();
        let (original_balance, new_balance) = changes.balance.take_both();

        let original_nonce = original_nonce.unwrap_or_default();
        let original_balance = original_balance.unwrap_or_default();

        let nonce = new_nonce.clone().unwrap_or(original_nonce.clone());
        let balance = new_balance.clone().unwrap_or(original_balance.clone());

        let bytecode = changes.bytecode.take().unwrap_or_else(|| {
            tracing::debug!("bytecode not set, defaulting to None");
            None
        });

        for (_, slot) in changes.slots {
            if let Some(slot) = slot.take_modified() {
                accounts_slots_changes.0.push(block_task.block_number.clone().as_i64());
                accounts_slots_changes.1.push(slot.index);
                accounts_slots_changes.2.push(changes.address.clone());
                accounts_slots_changes.3.push(slot.value.clone());
            }
        }

        accounts_changes.0.push(block_task.block_number.clone().as_i64());
        accounts_changes.1.push(changes.address.clone());
        accounts_changes.2.push(bytecode);
        accounts_changes.3.push(balance);
        accounts_changes.4.push(nonce);
        accounts_changes.5.push(changes.code_hash);
    }

    let mut transaction_batch = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut logs_batch = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for transaction in block_task.block_data.transactions {
        let last_char_vec  = transaction.input.hash.clone().into_hash_partition();

        for log in transaction.logs.iter() {
            logs_batch.0.push(transaction.block_number);
            logs_batch.1.push(transaction.input.hash.clone());
            logs_batch.2.push(log.log.address.clone());
            logs_batch.3.push(log.log_index);
            logs_batch.4.push(serde_json::to_value(log).unwrap());
            logs_batch.5.push(last_char_vec);
        }
        transaction_batch.0.push(transaction.block_number);
        transaction_batch.1.push(transaction.input.hash.clone());
        transaction_batch.2.push(serde_json::to_value(transaction).unwrap());
        transaction_batch.3.push(last_char_vec);
    }

    let pool_clone = Arc::<sqlx::Pool<sqlx::Postgres>>::clone(&pool);
    execute_with_retry(
        || async {
            let mut conn = pool_clone.acquire().await?;
            let mut tx = conn.begin().await?;

            sqlx::query!(
                "INSERT INTO neo_blocks (block_number, block_hash, block, account_changes, created_at) VALUES ($1, $2, $3, $4, NOW());",
                block_task.block_number as _,
                block_task.block_hash as _,
                block_data as _,
                account_changes as _,
            )
            .execute(&mut *tx)
            .await?;

            if !accounts_changes.0.is_empty() {
                sqlx::query!(
                    "INSERT INTO public.neo_accounts (block_number, address, bytecode, balance, nonce, code_hash)
                    SELECT * FROM UNNEST($1::bigint[], $2::bytea[], $3::bytea[], $4::numeric[], $5::numeric[], $6::bytea[])
                    AS t(block_number, address, bytecode, balance, nonce, code_hash);",
                    accounts_changes.0 as _,
                    accounts_changes.1 as _,
                    accounts_changes.2 as _,
                    accounts_changes.3 as _,
                    accounts_changes.4 as _,
                    accounts_changes.5 as _,
                )
                .execute(&mut *tx)
                .await?;
            }

            if !accounts_slots_changes.0.is_empty() {
                sqlx::query!(
                    "INSERT INTO public.neo_account_slots (block_number, slot_index, account_address, value)
                     SELECT * FROM UNNEST($1::bigint[], $2::bytea[], $3::bytea[], $4::bytea[])
                     AS t(block_number, slot_index, account_address, value);",
                    accounts_slots_changes.0 as _,
                    accounts_slots_changes.1 as _,
                    accounts_slots_changes.2 as _,
                    accounts_slots_changes.3 as _,
                )
                .execute(&mut *tx)
                .await?;
            }

            if !transaction_batch.0.is_empty() {
                sqlx::query!(
                    "INSERT INTO public.neo_transactions (block_number, hash, transaction_data, hash_partition)
                     SELECT * FROM UNNEST($1::bigint[], $2::bytea[], $3::jsonb[], $4::smallint[])
                     AS t(block_number, hash, transaction_data, hash_partition);",
                    transaction_batch.0 as _,
                    transaction_batch.1 as _,
                    transaction_batch.2 as _,
                    transaction_batch.3 as _,
                )
                .execute(&mut *tx)
                .await?;
            }

            if !logs_batch.0.is_empty() {
                sqlx::query!(
                    "INSERT INTO public.neo_logs (block_number, hash, address, log_idx, log_data, hash_partition)
                     SELECT * FROM UNNEST($1::bigint[], $2::bytea[], $3::bytea[], $4::numeric[], $5::jsonb[], $6::smallint[])
                     AS t(block_number, hash, address, log_idx, log_data, hash_partition);",
                    logs_batch.0 as _,
                    logs_batch.1 as _,
                    logs_batch.2 as _,
                    logs_batch.3 as _,
                    logs_batch.4 as _,
                    logs_batch.5 as _,
                )
                .execute(&mut *tx)
                .await?;
            }

            tx.commit().await?;
            Ok(())
        },
        3,
        Duration::from_millis(2),
    )
    .await
    .expect("Failed to commit after multiple attempts.");
}
