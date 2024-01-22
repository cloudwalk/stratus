use std::sync::Arc;

use dashmap::DashMap;
use itertools::Itertools;
use jsonrpsee::SubscriptionMessage;
use jsonrpsee::SubscriptionSink;
use tokio::sync::broadcast;
use tokio::time::sleep;
use tokio::time::Duration;

use crate::eth::primitives::Block;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;

/// Frequency of cleaning up closed subscriptions.
const CLEANING_FREQUENCY: Duration = Duration::from_secs(10);

/// Timeout used when sending notifications to subscribers.
const NOTIFICATION_TIMEOUT: Duration = Duration::from_secs(1);

type SubscriptionId = usize;


/// State of JSON-RPC websocket subscriptions.
#[derive(Debug, Default)]
pub struct RpcSubscriptions {
    /// Subscribers of `newHeads` event.
    pub new_heads: DashMap<SubscriptionId, SubscriptionSink>,

    /// Subscribers of `logs` event.
    pub logs: DashMap<SubscriptionId, (SubscriptionSink, LogFilter)>,
}

impl RpcSubscriptions {
    /// Spawns a new thread to clean up closed subscriptions from time to time.
    pub fn spawn_subscriptions_cleaner(self: Arc<Self>) {
        tokio::spawn(async move {
            loop {
                for closed_sub in self.new_heads.iter().filter(|sub| sub.is_closed()) {
                    self.new_heads.remove(closed_sub.key());
                }
                for closed_sub in self.logs.iter().filter(|sub| sub.0.is_closed()) {
                    self.logs.remove(closed_sub.key());
                }
                sleep(CLEANING_FREQUENCY).await;
            }
        });
    }

    /// Spawns a new thread that notifies subscribers about new created blocks.
    pub fn spawn_new_heads_notifier(self: Arc<Self>, mut rx: broadcast::Receiver<Block>) {
        tokio::spawn(async move {
            loop {
                let block = rx.recv().await.expect("newHeads notifier channel should never be closed");
                let msg = SubscriptionMessage::from(block.header);
                for sub in &self.new_heads {
                    notify(&sub, msg.clone()).await;
                }
            }
        });
    }

    /// Spawns a new thread that notifies subscribers about new transactions logs.
    pub fn spawn_logs_notifier(self: Arc<Self>, mut rx: broadcast::Receiver<LogMined>) {
        tokio::spawn(async move {
            loop {
                let log = rx.recv().await.expect("logs notifier channel should never be closed");

                let interested_subs = self.logs.iter().filter(|sub| sub.1.matches(&log)).collect_vec();
                let msg = SubscriptionMessage::from(log);
                for sub in interested_subs {
                    notify(&sub.0, msg.clone()).await;
                }
            }
        });
    }

    // -------------------------------------------------------------------------
    // Mutations
    // -------------------------------------------------------------------------

    /// Adds a new subscriber to `newHeads` event.
    pub fn add_new_heads(&self, sink: SubscriptionSink) {
        self.new_heads.insert(sink.connection_id(), sink);
    }

    /// Adds a new subscriber to `logs` event.
    pub fn add_logs(&self, sink: SubscriptionSink, filter: LogFilter) {
        self.logs.insert(sink.connection_id(), (sink, filter));
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
