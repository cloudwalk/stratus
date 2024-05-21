use std::collections::HashMap;
use std::sync::Arc;

use futures::join;
use itertools::Itertools;
use jsonrpsee::SubscriptionMessage;
use jsonrpsee::SubscriptionSink;
use tokio::sync::broadcast;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::eth::primitives::BlockHeader;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::ext::not;
use crate::if_else;
use crate::infra::metrics;

/// Frequency of cleaning up closed subscriptions.
const CLEANING_FREQUENCY: Duration = Duration::from_secs(10);

/// Timeout used when sending notifications to subscribers.
const NOTIFICATION_TIMEOUT: Duration = Duration::from_secs(1);

type SubscriptionId = usize;

/// State of JSON-RPC websocket subscriptions.
#[derive(Debug)]
pub struct RpcSubscriptions {
    pub connected: Arc<RpcSubscriptionsConnected>,
    pub handles: RpcSubscriptionsHandles,
}

impl RpcSubscriptions {
    /// Creates a new subscriptin manager that automatically spawns all necessary tasks in background.
    pub fn spawn(
        rx_pending_txs: broadcast::Receiver<Hash>,
        rx_blocks: broadcast::Receiver<BlockHeader>,
        rx_logs: broadcast::Receiver<LogMined>,
        cancellation: CancellationToken,
    ) -> Self {
        let connected = Arc::new(RpcSubscriptionsConnected::default());

        Self::spawn_subscriptions_cleaner(Arc::clone(&connected), cancellation.child_token());
        let handles = RpcSubscriptionsHandles {
            new_pending_txs: Self::spawn_new_pending_txs_notifier(Arc::clone(&connected), rx_pending_txs, cancellation.child_token()),
            new_heads: Self::spawn_new_heads_notifier(Arc::clone(&connected), rx_blocks, cancellation.child_token()),
            logs: Self::spawn_logs_notifier(Arc::clone(&connected), rx_logs, cancellation.child_token()),
        };

        Self { connected, handles }
    }

    /// Spawns a new task to clean up closed subscriptions from time to time.
    fn spawn_subscriptions_cleaner(subs: Arc<RpcSubscriptionsConnected>, cancellation: CancellationToken) -> JoinHandle<anyhow::Result<()>> {
        tracing::info!("spawning rpc subscriptions cleaner");

        tokio::task::Builder::new()
            .name("rpc::sub::cleaner")
            .spawn(async move {
                loop {
                    if cancellation.is_cancelled() {
                        tracing::warn!("exiting rpc subscription cleaner because of cancellation");
                        return Ok(());
                    }

                    // remove closed subscriptions
                    subs.new_pending_txs.write().await.retain(|_, sub| not(sub.is_closed()));
                    subs.new_heads.write().await.retain(|_, sub| not(sub.is_closed()));
                    subs.logs.write().await.retain(|_, (sub, _)| not(sub.is_closed()));

                    // update metrics
                    metrics::set_rpc_subscriptions(subs.new_pending_txs.read().await.len() as u64, "newPendingTransactions");
                    metrics::set_rpc_subscriptions(subs.new_heads.read().await.len() as u64, "newHeads");
                    metrics::set_rpc_subscriptions(subs.logs.read().await.len() as u64, "logs");

                    // await next iteration
                    tokio::time::sleep(CLEANING_FREQUENCY).await;
                }
            })
            .expect("spawning rpc subscription cleaner notifier should not fail")
    }

    /// Spawns a new task that notifies subscribers about new executed transactions.
    fn spawn_new_pending_txs_notifier(
        subs: Arc<RpcSubscriptionsConnected>,
        mut rx: broadcast::Receiver<Hash>,
        cancellation: CancellationToken,
    ) -> JoinHandle<anyhow::Result<()>> {
        tracing::info!("spawning rpc newPendingTransactions notifier");

        tokio::task::Builder::new()
            .name("rpc::sub::newPendingTransactions")
            .spawn(async move {
                loop {
                    if cancellation.is_cancelled() {
                        tracing::warn!("exiting rpc newPendingTransactions notifier because of cancellation");
                        return Ok(());
                    }

                    let Ok(hash) = rx.recv().await else {
                        tracing::warn!("stopping newPendingTransactions notifier because tx channel was closed");
                        break;
                    };

                    let subs = subs.new_pending_txs.read().await;
                    Self::notify(subs.values(), hash.to_string()).await;
                }
                Ok(())
            })
            .expect("spawning rpc newPendingTransactions notifier should not fail")
    }

    /// Spawns a new task that notifies subscribers about new created blocks.
    fn spawn_new_heads_notifier(
        subs: Arc<RpcSubscriptionsConnected>,
        mut rx: broadcast::Receiver<BlockHeader>,
        cancellation: CancellationToken,
    ) -> JoinHandle<anyhow::Result<()>> {
        tracing::info!("spawning rpc newHeads notifier");

        tokio::task::Builder::new()
            .name("rpc::sub::newHeads")
            .spawn(async move {
                loop {
                    if cancellation.is_cancelled() {
                        tracing::warn!("exiting rpc newHeads notifier because of cancellation");
                        return Ok(());
                    }

                    let Ok(header) = rx.recv().await else {
                        tracing::warn!("stopping newHeads notifier because tx channel was closed");
                        break;
                    };

                    let subs = subs.new_heads.read().await;
                    Self::notify(subs.values(), header).await;
                }
                Ok(())
            })
            .expect("spawning rpc newHeads notifier should not fail")
    }

    /// Spawns a new task that notifies subscribers about new transactions logs.
    fn spawn_logs_notifier(
        subs: Arc<RpcSubscriptionsConnected>,
        mut rx: broadcast::Receiver<LogMined>,
        cancellation: CancellationToken,
    ) -> JoinHandle<anyhow::Result<()>> {
        tracing::info!("spawning rpc logs notifier");

        tokio::task::Builder::new()
            .name("rpc::sub::logs")
            .spawn(async move {
                loop {
                    if cancellation.is_cancelled() {
                        tracing::warn!("exiting rpc logs cleaner because of cancellation");
                        return Ok(());
                    }

                    let Ok(log) = rx.recv().await else {
                        tracing::warn!("stopping logs notifier because tx channel was closed");
                        break;
                    };

                    let subs = subs.logs.read().await;
                    let interested_subs = subs
                        .values()
                        .filter_map(|(sub, filter)| if_else!(filter.matches(&log), Some(sub), None))
                        .collect_vec();

                    Self::notify(interested_subs.into_iter(), log).await;
                }
                Ok(())
            })
            .expect("spawning rpc logs notifier should not fail")
    }

    async fn notify(subs: impl ExactSizeIterator<Item = &SubscriptionSink>, msg: impl Into<SubscriptionMessage>) {
        if subs.len() == 0 {
            return;
        }

        let msg = msg.into();
        for sub in subs {
            if sub.is_closed() {
                continue;
            }
            if let Err(e) = sub.send_timeout(msg.clone(), NOTIFICATION_TIMEOUT).await {
                tracing::error!(reason = ?e, "failed to send subscription notification");
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Notifier handles
// -----------------------------------------------------------------------------

/// Handles of subscription background tasks.
#[derive(Debug)]
pub struct RpcSubscriptionsHandles {
    new_pending_txs: JoinHandle<anyhow::Result<()>>,
    new_heads: JoinHandle<anyhow::Result<()>>,
    logs: JoinHandle<anyhow::Result<()>>,
}

impl RpcSubscriptionsHandles {
    pub async fn stopped(self) {
        let _ = join!(self.new_pending_txs, self.new_heads, self.logs);
    }
}

// -----------------------------------------------------------------------------
// Connected clients
// -----------------------------------------------------------------------------

/// Active client subscriptions.
#[derive(Debug, Default)]
pub struct RpcSubscriptionsConnected {
    new_pending_txs: RwLock<HashMap<SubscriptionId, SubscriptionSink>>,
    new_heads: RwLock<HashMap<SubscriptionId, SubscriptionSink>>,
    logs: RwLock<HashMap<SubscriptionId, (SubscriptionSink, LogFilter)>>,
}

impl RpcSubscriptionsConnected {
    /// Adds a new subscriber to `newPendingTransactions` event.
    pub async fn add_new_pending_txs(&self, sink: SubscriptionSink) {
        tracing::debug!(id = %sink.connection_id(), "subscribing to newPendingTransactions event");
        self.new_pending_txs.write().await.insert(sink.connection_id(), sink);
    }

    /// Adds a new subscriber to `newHeads` event.
    pub async fn add_new_heads(&self, sink: SubscriptionSink) {
        tracing::debug!(id = %sink.connection_id(), "subscribing to newHeads event");
        self.new_heads.write().await.insert(sink.connection_id(), sink);
    }

    /// Adds a new subscriber to `logs` event.
    pub async fn add_logs(&self, sink: SubscriptionSink, filter: LogFilter) {
        tracing::debug!(id = %sink.connection_id(), ?filter, "subscribing to logs event");
        self.logs.write().await.insert(sink.connection_id(), (sink, filter));
    }
}
