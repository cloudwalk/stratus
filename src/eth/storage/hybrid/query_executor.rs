use sqlx::{Pool, Postgres, Acquire, Transaction};
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use crate::eth::primitives::{Address, Bytes, Nonce, Wei};
use super::BlockTask;

type BlockNumbers = Vec<i64>;
type Addresses = Vec<Address>;
type OptionalBytes = Vec<Option<Bytes>>;
type Weis = Vec<Wei>;
type Nonces = Vec<Nonce>;

type AccountChanges = (BlockNumbers, Addresses, OptionalBytes, Weis, Nonces);

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
    let mut accounts_changes: AccountChanges = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());

   let pool_clone = pool.clone();
    execute_with_retry(|| async {
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

        sqlx::query!(
            "INSERT INTO public.neo_accounts (block_number, address, bytecode, balance, nonce)
             SELECT * FROM UNNEST($1::bigint[], $2::bytea[], $3::bytea[], $4::numeric[], $5::numeric[])
             AS t(block_number, address, bytecode, balance, nonce);",
            accounts_changes.0 as _,
            accounts_changes.1 as _,
            accounts_changes.2 as _,
            accounts_changes.3 as _,
            accounts_changes.4 as _,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }, 3, Duration::from_millis(2)).await.expect("Failed to commit after multiple attempts.");
}
