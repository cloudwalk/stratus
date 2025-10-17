mod fetchers;
pub(crate) mod importer_config;
#[allow(clippy::module_inception)]
mod importer_supervisor;
mod importers;
use std::borrow::Cow;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::bail;
pub use importer_config::ImporterConfig;
pub use importer_supervisor::ImporterConsensus;
use itertools::Itertools;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::Span;

use crate::GlobalState;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExecutionChanges;
use crate::eth::storage::StratusStorage;
use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;
use crate::ext::DisplayExt;
use crate::ext::SleepReason;
use crate::ext::traced_sleep;
use crate::globals::IMPORTER_ONLINE_TASKS_SEMAPHORE;
use crate::infra::BlockchainClient;
use crate::infra::kafka::KafkaConnector;
#[cfg(feature = "metrics")]
use crate::infra::metrics;
use crate::infra::tracing::SpanExt;
use crate::ledger::events::transaction_to_events;
use crate::log_and_err;

#[derive(Clone, Copy)]
pub enum ImporterMode {
    /// A normal follower imports a mined block.
    ReexecutionFollower,
    /// Fake leader feches a block, re-executes its txs and then mines it's own block.
    FakeLeader,
    /// Fetch a block with pre-computed changes
    BlockWithChanges,
}

// -----------------------------------------------------------------------------
// Globals
// -----------------------------------------------------------------------------

/// Current block number of the external RPC blockchain.
static EXTERNAL_RPC_CURRENT_BLOCK: AtomicU64 = AtomicU64::new(0);

/// Timestamp of when EXTERNAL_RPC_CURRENT_BLOCK was updated last.
static LATEST_FETCHED_BLOCK_TIME: AtomicU64 = AtomicU64::new(0);

/// Only sets the external RPC current block number if it is equals or greater than the current one.
fn set_external_rpc_current_block(new_number: BlockNumber) {
    LATEST_FETCHED_BLOCK_TIME.store(chrono::Utc::now().timestamp() as u64, Ordering::Relaxed);
    let _ = EXTERNAL_RPC_CURRENT_BLOCK.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current_number| {
        Some(current_number.max(new_number.as_u64()))
    });
}

// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------

/// Timeout awaiting for newHeads event before fallback to polling.
const TIMEOUT_NEW_HEADS: Duration = Duration::from_millis(2000);
pub const TASKS_COUNT: usize = 3;

async fn receive_with_timeout<T>(rx: &mut mpsc::Receiver<T>) -> anyhow::Result<Option<T>> {
    match timeout(Duration::from_secs(2), rx.recv()).await {
        Ok(Some(inner)) => Ok(Some(inner)),
        Ok(None) => bail!("channel closed"),
        Err(_timed_out) => {
            tracing::warn!(timeout = "2s", "timeout reading block executor channel, expected around 1 block per second");
            Ok(None)
        }
    }
}

/// Send block transactions to Kafka
async fn send_block_to_kafka(kafka_connector: &Option<KafkaConnector>, block: &Block) -> anyhow::Result<()> {
    if let Some(kafka_conn) = kafka_connector {
        let events = block
            .transactions
            .iter()
            .flat_map(|tx| transaction_to_events(block.header.timestamp, Cow::Borrowed(tx)));

        kafka_conn.send_buffered(events, 50).await?;
    }
    Ok(())
}

/// Record metrics for imported block
#[cfg(feature = "metrics")]
fn record_import_metrics(block_tx_len: usize, start: std::time::Instant) {
    metrics::inc_n_importer_online_transactions_total(block_tx_len as u64);
    metrics::inc_import_online_mined_block(start.elapsed());
}

// -----------------------------------------------------------------------------
// Number fetcher
// -----------------------------------------------------------------------------

/// Retrieves the blockchain current block number.
async fn start_number_fetcher(chain: Arc<BlockchainClient>, sync_interval: Duration) -> anyhow::Result<()> {
    const TASK_NAME: &str = "external-number-fetcher";
    let _permit = IMPORTER_ONLINE_TASKS_SEMAPHORE.acquire().await;

    // initial newHeads subscriptions.
    // abort application if cannot subscribe.
    let mut sub_new_heads = if chain.supports_ws() {
        tracing::info!("{} subscribing to newHeads event", TASK_NAME);

        match chain.subscribe_new_heads().await {
            Ok(sub) => {
                tracing::info!("{} subscribed to newHeads events", TASK_NAME);
                Some(sub)
            }
            Err(e) => {
                let message = GlobalState::shutdown_from(TASK_NAME, "cannot subscribe to newHeads event");
                return log_and_err!(reason = e, message);
            }
        }
    } else {
        tracing::warn!("{} blockchain client does not have websocket enabled", TASK_NAME);
        None
    };

    // keep reading websocket subscription or polling via http.
    loop {
        if should_shutdown(TASK_NAME) {
            return Ok(());
        }

        // if we have a subscription, try to read from subscription.
        // in case of failure, re-subscribe because current subscription may have been closed in the server.
        if let Some(sub) = &mut sub_new_heads {
            tracing::info!("{} awaiting block number from newHeads subscription", TASK_NAME);
            match timeout(TIMEOUT_NEW_HEADS, sub.next()).await {
                Ok(Some(Ok(block))) => {
                    tracing::info!(block_number = %block.number(), "{} received newHeads event", TASK_NAME);
                    set_external_rpc_current_block(block.number());
                    continue;
                }
                Ok(None) =>
                    if !should_shutdown(TASK_NAME) {
                        tracing::error!("{} newHeads subscription closed by the other side", TASK_NAME);
                    },
                Ok(Some(Err(e))) =>
                    if !should_shutdown(TASK_NAME) {
                        tracing::error!(reason = ?e, "{} failed to read newHeads subscription event", TASK_NAME);
                    },
                Err(_) =>
                    if !should_shutdown(TASK_NAME) {
                        tracing::error!("{} timed-out waiting for newHeads subscription event", TASK_NAME);
                    },
            }

            if should_shutdown(TASK_NAME) {
                return Ok(());
            }

            // resubscribe if necessary.
            // only update the existing subscription if succedeed, otherwise we will try again in the next iteration.
            if chain.supports_ws() {
                tracing::info!("{} resubscribing to newHeads event", TASK_NAME);
                match chain.subscribe_new_heads().await {
                    Ok(sub) => {
                        tracing::info!("{} resubscribed to newHeads event", TASK_NAME);
                        sub_new_heads = Some(sub);
                    }
                    Err(e) =>
                        if !should_shutdown(TASK_NAME) {
                            tracing::error!(reason = ?e, "{} failed to resubscribe to newHeads event", TASK_NAME);
                        },
                }
            }
        }

        if should_shutdown(TASK_NAME) {
            return Ok(());
        }

        // fallback to polling
        tracing::warn!("{} falling back to http polling because subscription failed or it is not enabled", TASK_NAME);
        match chain.fetch_block_number().await {
            Ok(block_number) => {
                tracing::info!(
                    %block_number,
                    sync_interval = %sync_interval.to_string_ext(),
                    "fetched current block number via http. awaiting sync interval to retrieve again."
                );
                set_external_rpc_current_block(block_number);
                traced_sleep(sync_interval, SleepReason::SyncData).await;
            }
            Err(e) =>
                if !should_shutdown(TASK_NAME) {
                    tracing::error!(reason = ?e, "failed to retrieve block number. retrying now.");
                },
        }
    }
}

fn should_shutdown(task_name: &str) -> bool {
    GlobalState::is_shutdown_warn(task_name) || GlobalState::is_importer_shutdown_warn(task_name)
}

fn create_execution_changes(storage: &Arc<StratusStorage>, changes: BlockChangesRocksdb) -> anyhow::Result<ExecutionChanges> {
    let addresses = changes.account_changes.keys().copied().map_into().collect_vec();
    let accounts = storage.perm.read_accounts(addresses)?;
    let mut exec_changes = changes.to_incomplete_execution_changes();
    for (addr, acc) in accounts {
        match exec_changes.accounts.entry(addr) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                let item = entry.get_mut();
                item.apply_original(acc);
            }
            std::collections::hash_map::Entry::Vacant(_) => unreachable!("we got the addresses from the changes"),
        }
    }
    Ok(exec_changes)
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

/// Generic retry logic for fetching data from blockchain
async fn fetch_with_retry<T, F, Fut>(block_number: BlockNumber, fetch_fn: F, operation_name: &str) -> T
where
    F: Fn(BlockNumber) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<Option<T>>>,
{
    const RETRY_DELAY: Duration = Duration::from_millis(10);
    Span::with(|s| {
        s.rec_str("block_number", &block_number);
    });

    loop {
        tracing::info!(%block_number, "fetching {}", operation_name);

        match fetch_fn(block_number).await {
            Ok(Some(response)) => return response,
            Ok(None) => {
                tracing::warn!(
                    %block_number,
                    delay_ms = %RETRY_DELAY.as_millis(),
                    "{} not available yet, retrying with delay.",
                    operation_name
                );
                traced_sleep(RETRY_DELAY, SleepReason::RetryBackoff).await;
            }
            Err(e) => {
                tracing::warn!(
                    reason = ?e,
                    %block_number,
                    delay_ms = %RETRY_DELAY.as_millis(),
                    "failed to fetch {}, retrying with delay.",
                    operation_name
                );
                traced_sleep(RETRY_DELAY, SleepReason::RetryBackoff).await;
            }
        };
    }
}
