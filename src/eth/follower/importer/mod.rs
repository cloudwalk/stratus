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
use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::Span;

use crate::GlobalState;
use crate::eth::rpc::BlockchainClient;
use crate::eth::types::Block;
use crate::eth::types::BlockNumber;
use crate::ext::DisplayExt;
use crate::ext::SleepReason;
use crate::ext::traced_sleep;
use crate::globals::IMPORTER_ONLINE_TASKS_SEMAPHORE;
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
pub async fn send_block_to_kafka(kafka_connector: &Option<KafkaConnector>, block: &Block) -> anyhow::Result<()> {
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
fn record_import_metrics(block_tx_len: usize, duration: std::time::Duration) {
    metrics::inc_n_importer_online_transactions_total(block_tx_len as u64);
    metrics::inc_import_online_mined_block(duration);
}

#[cfg(not(feature = "metrics"))]
fn record_import_metrics(_block_tx_len: usize, _duration: std::time::Duration) {}

/// Record metrics for fetched block
#[cfg(feature = "metrics")]
fn record_fetch_metrics(fetch_duration: std::time::Duration, post_process_duration: std::time::Duration) {
    metrics::inc_import_online_fetched_block(fetch_duration);
    metrics::inc_import_online_post_process_block(post_process_duration);
    metrics::inc_import_online_fetch_and_post_process_block(fetch_duration + post_process_duration);
}

#[cfg(not(feature = "metrics"))]
fn record_fetch_metrics(_fetch_duration: std::time::Duration, _post_process_duration: std::time::Duration) {}

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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use hash_hasher::HashBuildHasher;

    use crate::eth::executor::AccountChanges;
    use crate::eth::executor::ChangeValue;
    use crate::eth::executor::ExecutionChanges;
    use crate::eth::executor::ExecutionResult;
    use crate::eth::executor::TransactionExecution;
    use crate::eth::executor::TransactionExecutionInput;
    use crate::eth::executor::TransactionExecutionOutput;
    use crate::eth::follower::importer::fetchers::DataFetcher;
    use crate::eth::follower::importer::fetchers::block_with_changes::BlockWithChangesFetcher;
    use crate::eth::follower::importer::importers::ImporterWorker;
    use crate::eth::follower::importer::importers::replication::ReplicationWorker;
    use crate::eth::miner::Miner;
    use crate::eth::miner::MinerMode;
    use crate::eth::rpc::BlockchainClient;
    use crate::eth::storage::ExecutionKind;
    use crate::eth::storage::StratusStorage;
    use crate::eth::storage::permanent::rocks::types::AccountChangesRocksdb;
    use crate::eth::storage::permanent::rocks::types::AddressRocksdb;
    use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;
    use crate::eth::storage::permanent::rocks::types::BlockRocksdb;
    use crate::eth::types::Account;
    use crate::eth::types::Address;
    use crate::eth::types::Block;
    use crate::eth::types::BlockNumber;
    use crate::eth::types::PointInTime;
    use crate::eth::types::Signature;
    use crate::eth::types::TransactionInfo;
    use crate::eth::types::TransactionInput;
    use crate::eth::types::UnixTime;
    use crate::eth::types::Wei;

    impl AccountChanges {
        pub fn from_changed(account: Account) -> Self {
            Self {
                nonce: ChangeValue::Changed(account.nonce),
                balance: ChangeValue::Changed(account.balance),
                bytecode: ChangeValue::Changed(account.bytecode),
            }
        }
    }

    /// Mines a block applying `changes` (mirrors the helper in `stratus_storage` tests).
    fn mine_block(storage: &StratusStorage, changes: ExecutionChanges) {
        let (header, _) = storage.read_pending_block_header();
        let evm_input = TransactionExecutionInput::from_eth_transaction(&TransactionInput::default(), header.number, *header.timestamp);

        let result = TransactionExecutionOutput {
            result: ExecutionResult::Success,
            changes,
            ..Default::default()
        };

        let tx = TransactionExecution::new(TransactionInfo::default(), Signature::default(), evm_input, result);
        storage.save_execution(tx).expect("save execution");

        let (block, block_changes) = storage.finish_pending_block().expect("finish pending block");
        storage.save_block(block.into(), block_changes).expect("save block");
    }

    /// Builds `ExecutionChanges` that set `address`'s balance to `balance` (nonce/bytecode untouched).
    fn balance_changes(address: Address, balance: Wei) -> ExecutionChanges {
        let mut changes = ExecutionChanges::default();
        changes
            .accounts
            .insert(address, AccountChanges::from_changed(Account::new_with_balance(address, balance)));
        changes
    }

    /// Builds the leader-side `BlockChangesRocksdb` for a block where `address` had its nonce
    /// changed but its balance left untouched (balance entry is `None`).
    fn block_changes_nonce_only(address: Address) -> BlockChangesRocksdb {
        let mut account_changes = HashMap::with_hasher(HashBuildHasher::default());
        account_changes.insert(
            AddressRocksdb::from(address),
            AccountChangesRocksdb {
                balance: None,
                nonce: Some(5u64.into()),
                bytecode: None,
            },
        );
        BlockChangesRocksdb {
            account_changes,
            slot_changes: HashMap::with_hasher(HashBuildHasher::default()),
        }
    }

    /// Replication importer (`BlockWithChangesFetcher` + `ReplicationWorker`) must commit changes
    /// whose unchanged account fields reflect the block's pre-state, not whatever happens to be
    /// latest in permanent storage when the fetcher post-processes the block.
    ///
    /// The fetcher pipeline runs ahead of the importer: block N is post-processed while the
    /// importer has only committed up to block N-k. Completion must therefore happen at import
    /// time (when perm is caught up to block N-1), not at post-process time. `FakeLeaderWorker`
    /// already does this via `expected_changes.complete(self.storage.as_ref())`; `ReplicationWorker`
    /// must do the same.
    ///
    /// This test drives the combo at the importer level: `post_process` produces
    /// `ExecutionChanges<Incomplete>` (perm-independent), then `ReplicationWorker::import` must
    /// complete them against the now-caught-up perm. It fails to compile until `ReplicationWorker`
    /// accepts `ExecutionChanges<Incomplete>` and completes internally; once it does, the assertion
    /// passes (committed `200` = block 3's pre-state).
    #[tokio::test]
    async fn replication_importer_commits_stale_completed_changes_when_fetcher_runs_ahead() {
        let storage = Arc::new(StratusStorage::new_test().expect("failed to build test storage"));
        let miner = Arc::new(Miner::new(Arc::clone(&storage), MinerMode::External));
        let worker = ReplicationWorker {
            miner,
            kafka_connector: None,
            storage: Arc::clone(&storage),
        };
        // `post_process` never touches the chain; the client only satisfies the fetcher's `chain` field.
        let chain = Arc::new(BlockchainClient::new_without_health_check("http://127.0.0.1:9/").expect("build test client"));
        let fetcher = BlockWithChangesFetcher { chain };

        let address = Address::new([0xCC; 20]);

        // Block 1: B.balance = 100. permanent storage is now at block 1.
        mine_block(&storage, balance_changes(address, Wei::from(100u64)));

        // The fetcher post-processes block 3 while the importer is still at block 1 (fetcher ahead).
        // Block 3 changed B's nonce but left its balance untouched (balance entry is `None`).
        // `post_process` returns `ExecutionChanges<Incomplete>` — it does NOT read perm, so the
        // fetcher being ahead does not corrupt the changes.
        let fetched_3 = (
            BlockRocksdb::from(Block::new(BlockNumber::from(3u64), UnixTime::from(0u64))),
            block_changes_nonce_only(address),
        );
        let (block_3, changes_3) = fetcher.post_process(fetched_3).await.expect("post_process");

        // Intervening block 2: B.balance = 200. This is the correct pre-state for block 3.
        // permanent storage is now at block 2.
        mine_block(&storage, balance_changes(address, Wei::from(200u64)));

        // The importer imports block 3. `ReplicationWorker::import` must complete `changes_3`
        // (Incomplete) against perm at import time, when perm is caught up to block 2 (200).
        worker.import((block_3, changes_3)).await.expect("import");

        // Block 3 did not change B.balance, so the committed value must equal block 3's pre-state
        // (block 2 = 200). Completing at import time (perm caught up) yields 200; completing at
        // post-process time (perm behind) would yield the stale 100.
        let account = storage.read_account(address, ExecutionKind::RPC(PointInTime::Latest)).expect("read account");
        assert_eq!(
            account.balance,
            Wei::from(200u64),
            "replication importer did not complete changes against the block's pre-state"
        );
    }
}
