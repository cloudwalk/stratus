use std::sync::Arc;

use dashmap::mapref::multiple::RefMulti;
use dashmap::DashMap;
use jsonrpsee::SubscriptionMessage;
use jsonrpsee::SubscriptionSink;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio::time::Duration;

use crate::eth::primitives::Block;
use crate::eth::primitives::LogMined;

/// Frequency of cleaning up closed subscriptions.
const CLEANING_FREQUENCY: Duration = Duration::from_secs(10);

/// Timeout used when sending notifications to subscribers.
const NOTIFICATION_TIMEOUT: Duration = Duration::from_secs(1);

pub type RpcBlockNotifier = mpsc::UnboundedSender<Block>;
pub type RpcLogNotifier = mpsc::UnboundedSender<LogMined>;

#[derive(Debug, Default)]
pub struct RpcSubscriptions {
    pub new_heads: DashMap<usize, SubscriptionSink>,
    pub logs: DashMap<usize, SubscriptionSink>,
}

impl RpcSubscriptions {
    // -------------------------------------------------------------------------
    // Background tasks
    // -------------------------------------------------------------------------

    /// Spawns a new thread to clean up closed subscriptions from time to time.
    pub fn spawn_subscriptions_cleaner(self: Arc<Self>) {
        fn remove_closed_subs(map: &DashMap<usize, SubscriptionSink>) {
            for closed_sub in map.iter().filter(|sub| sub.is_closed()) {
                map.remove(closed_sub.key());
            }
        }
        tokio::spawn(async move {
            loop {
                remove_closed_subs(&self.new_heads);
                sleep(CLEANING_FREQUENCY).await;
            }
        });
    }

    /// Spawns a new thread that notifies subscribers about new heads.
    pub fn spawn_new_heads_notifier(self: Arc<Self>) -> RpcBlockNotifier {
        let (tx, mut rx) = mpsc::unbounded_channel::<Block>();
        tokio::spawn(async move {
            loop {
                let block = rx.recv().await.expect("newHeads notifier channel should never be closed");
                let msg: SubscriptionMessage = block.header.into();
                for sub in self.new_heads.iter() {
                    notify(sub, msg.clone()).await;
                }
            }
        });
        tx
    }

    /// Spawns a new thread that notifies subscribers about transactions logs.
    ///
    /// TODO: must consider filters.
    pub fn spawn_logs_notifier(self: Arc<Self>) -> RpcLogNotifier {
        let (tx, mut rx) = mpsc::unbounded_channel::<LogMined>();
        tokio::spawn(async move {
            loop {
                let log = rx.recv().await.expect("logs notifier channel should never be closed");
                let msg: SubscriptionMessage = log.into();
                for sub in self.logs.iter() {
                    notify(sub, msg.clone()).await;
                }
            }
        });
        tx
    }

    // -------------------------------------------------------------------------
    // Mutations
    // -------------------------------------------------------------------------

    /// Adds a new subscriber to `newHeads` event.
    pub fn add_new_heads(&self, sink: SubscriptionSink) {
        self.new_heads.insert(sink.connection_id(), sink);
    }

    /// Adds a new subscriber to `logs` event.
    pub fn add_logs(&self, sink: SubscriptionSink) {
        self.logs.insert(sink.connection_id(), sink);
    }
}

async fn notify(sub: RefMulti<'_, usize, SubscriptionSink>, msg: SubscriptionMessage) {
    if sub.is_closed() {
        return;
    }
    if let Err(e) = sub.send_timeout(msg, NOTIFICATION_TIMEOUT).await {
        tracing::error!(reason = ?e, "failed to send subscription notification");
    }
}
