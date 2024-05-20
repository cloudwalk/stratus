use std::collections::HashMap;
use std::sync::Arc;

use itertools::Itertools;
use jsonrpsee::SubscriptionMessage;
use jsonrpsee::SubscriptionSink;
use tokio::sync::broadcast;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio::time::Duration;

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
#[derive(Debug, Default)]
pub struct RpcSubscriptions {
    /// Subscribers of `newPendingTransactions` event.
    pub new_pending_txs: RwLock<HashMap<SubscriptionId, SubscriptionSink>>,

    /// Subscribers of `newHeads` event.
    pub new_heads: RwLock<HashMap<SubscriptionId, SubscriptionSink>>,

    /// Subscribers of `logs` event.
    pub logs: RwLock<HashMap<SubscriptionId, (SubscriptionSink, LogFilter)>>,
}

impl RpcSubscriptions {
    /// Spawns a new task to clean up closed subscriptions from time to time.
    pub fn spawn_subscriptions_cleaner(self: Arc<Self>) -> JoinHandle<anyhow::Result<()>> {
        tracing::info!("spawning rpc subscriptions cleaner");

        tokio::spawn(async move {
            loop {
                // remove closed subscriptions
                self.new_pending_txs.write().await.retain(|_, sub| not(sub.is_closed()));
                self.new_heads.write().await.retain(|_, sub| not(sub.is_closed()));
                self.logs.write().await.retain(|_, (sub, _)| not(sub.is_closed()));

                // update metrics
                metrics::set_rpc_subscriptions(self.new_pending_txs.read().await.len() as u64, "newPendingTransactions");
                metrics::set_rpc_subscriptions(self.new_heads.read().await.len() as u64, "newHeads");
                metrics::set_rpc_subscriptions(self.logs.read().await.len() as u64, "logs");

                // await next iteration
                tokio::time::sleep(CLEANING_FREQUENCY).await;
            }
        })
    }

    /// Spawns a new task that notifies subscribers about new executed transactions.
    pub fn spawn_new_pending_txs_notifier(self: Arc<Self>, mut rx: broadcast::Receiver<Hash>) -> JoinHandle<anyhow::Result<()>> {
        tracing::info!("spawning rpc newPendingTransactions notifier");

        tokio::spawn(async move {
            loop {
                let Ok(hash) = rx.recv().await else {
                    tracing::warn!("stopping newPendingTransactions notifier because tx channel was closed");
                    break;
                };

                let subs = self.new_pending_txs.read().await;
                notify(subs.values(), SubscriptionMessage::from(hash.to_string())).await;
            }
            Ok(())
        })
    }

    /// Spawns a new task that notifies subscribers about new created blocks.
    pub fn spawn_new_heads_notifier(self: Arc<Self>, mut rx: broadcast::Receiver<BlockHeader>) -> JoinHandle<anyhow::Result<()>> {
        tracing::info!("spawning rpc newHeads notifier");

        tokio::spawn(async move {
            loop {
                let Ok(header) = rx.recv().await else {
                    tracing::warn!("stopping newHeads notifier because tx channel was closed");
                    break;
                };

                let subs = self.new_heads.read().await;
                notify(subs.values(), SubscriptionMessage::from(header)).await;
            }
            Ok(())
        })
    }

    /// Spawns a new task that notifies subscribers about new transactions logs.
    pub fn spawn_logs_notifier(self: Arc<Self>, mut rx: broadcast::Receiver<LogMined>) -> JoinHandle<anyhow::Result<()>> {
        tracing::info!("spawning rpc logs notifier");

        tokio::spawn(async move {
            loop {
                let Ok(log) = rx.recv().await else {
                    tracing::warn!("stopping logs notifier because tx channel was closed");
                    break;
                };

                let subs = self.logs.read().await;
                let interested_subs = subs
                    .values()
                    .filter_map(|(sub, filter)| if_else!(filter.matches(&log), Some(sub), None))
                    .collect_vec();

                notify(interested_subs, SubscriptionMessage::from(log)).await;
            }
            Ok(())
        })
    }

    // -------------------------------------------------------------------------
    // Mutations
    // -------------------------------------------------------------------------

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

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------
async fn notify(subs: impl IntoIterator<Item = &SubscriptionSink>, msg: SubscriptionMessage) {
    for sub in subs {
        if sub.is_closed() {
            continue;
        }
        if let Err(e) = sub.send_timeout(msg.clone(), NOTIFICATION_TIMEOUT).await {
            tracing::error!(reason = ?e, "failed to send subscription notification");
        }
    }
}
