use std::collections::HashMap;
use std::sync::Arc;

use itertools::Itertools;
use jsonrpsee::SubscriptionMessage;
use jsonrpsee::SubscriptionSink;
use tokio::sync::broadcast;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio::time::Duration;

use crate::eth::primitives::Block;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::ext::not;

/// Frequency of cleaning up closed subscriptions.
const CLEANING_FREQUENCY: Duration = Duration::from_secs(10);

/// Timeout used when sending notifications to subscribers.
const NOTIFICATION_TIMEOUT: Duration = Duration::from_secs(1);

type SubscriptionId = usize;

/// State of JSON-RPC websocket subscriptions.
#[derive(Debug, Default)]
pub struct RpcSubscriptions {
    /// Subscribers of `newHeads` event.
    pub new_heads: RwLock<HashMap<SubscriptionId, SubscriptionSink>>,

    /// Subscribers of `logs` event.
    pub logs: RwLock<HashMap<SubscriptionId, (SubscriptionSink, LogFilter)>>,
}

impl RpcSubscriptions {
    /// Spawns a new thread to clean up closed subscriptions from time to time.
    pub fn spawn_subscriptions_cleaner(self: Arc<Self>) -> JoinHandle<anyhow::Result<()>> {
        tokio::spawn(async move {
            loop {
                let any_new_heads_closed = self.new_heads.read().await.iter().any(|(_, sub)| sub.is_closed());
                if any_new_heads_closed {
                    let mut new_heads_subs = self.new_heads.write().await;
                    let before = new_heads_subs.len();
                    new_heads_subs.retain(|_, sub| not(sub.is_closed()));
                    tracing::info!(%before, after = new_heads_subs.len(), "removed newHeads subscriptions");
                }

                let any_logs_closed = self.logs.read().await.iter().any(|(_, (sub, _))| sub.is_closed());
                if any_logs_closed {
                    let mut logs_subs = self.logs.write().await;
                    let before = logs_subs.len();
                    logs_subs.retain(|_, (sub, _)| not(sub.is_closed()));
                    tracing::info!(%before, after = logs_subs.len(), "removed logs subscriptions");
                }

                tokio::time::sleep(CLEANING_FREQUENCY).await;
            }
        })
    }

    /// Spawns a new thread that notifies subscribers about new created blocks.
    pub fn spawn_new_heads_notifier(self: Arc<Self>, mut rx: broadcast::Receiver<Block>) -> JoinHandle<anyhow::Result<()>> {
        tokio::spawn(async move {
            loop {
                let Ok(block) = rx.recv().await else {
                    tracing::warn!("stopping subscription blocks notifier because tx channel was closed");
                    break;
                };
                let msg = SubscriptionMessage::from(block.header);

                let new_heads_subs = self.new_heads.read().await;
                for sub in new_heads_subs.values() {
                    notify(sub, msg.clone()).await;
                }
            }
            Ok(())
        })
    }

    /// Spawns a new thread that notifies subscribers about new transactions logs.
    pub fn spawn_logs_notifier(self: Arc<Self>, mut rx: broadcast::Receiver<LogMined>) -> JoinHandle<anyhow::Result<()>> {
        tokio::spawn(async move {
            loop {
                let Ok(log) = rx.recv().await else {
                    tracing::warn!("stopping subscription logs notifier because tx channel was closed");
                    break;
                };

                let logs_subs = self.logs.read().await;
                let logs_interested_subs = logs_subs.values().filter(|(_, filter)| filter.matches(&log)).collect_vec();

                let msg = SubscriptionMessage::from(log);
                for sub in logs_interested_subs {
                    notify(&sub.0, msg.clone()).await;
                }
            }
            Ok(())
        })
    }

    // -------------------------------------------------------------------------
    // Mutations
    // -------------------------------------------------------------------------

    /// Adds a new subscriber to `newHeads` event.
    pub async fn add_new_heads(&self, sink: SubscriptionSink) {
        self.new_heads.write().await.insert(sink.connection_id(), sink);
    }

    /// Adds a new subscriber to `logs` event.
    pub async fn add_logs(&self, sink: SubscriptionSink, filter: LogFilter) {
        self.logs.write().await.insert(sink.connection_id(), (sink, filter));
    }
}

async fn notify(sub: &SubscriptionSink, msg: SubscriptionMessage) {
    if sub.is_closed() {
        return;
    }
    if let Err(e) = sub.send_timeout(msg, NOTIFICATION_TIMEOUT).await {
        tracing::error!(reason = ?e, "failed to send subscription notification");
    }
}
