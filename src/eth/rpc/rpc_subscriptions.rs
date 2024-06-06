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

use crate::channel_read;
use crate::eth::primitives::BlockHeader;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::ext::named_spawn;
use crate::ext::not;
use crate::ext::traced_sleep;
use crate::if_else;
#[cfg(feature = "metrics")]
use crate::infra::metrics;
use crate::infra::tracing::warn_task_tx_closed;
use crate::GlobalState;

/// Frequency of cleaning up closed subscriptions.
const CLEANING_FREQUENCY: Duration = Duration::from_secs(10);

/// Timeout used when sending notifications to subscribers.
const NOTIFICATION_TIMEOUT: Duration = Duration::from_secs(1);

type SubscriptionId = usize;

#[cfg(feature = "metrics")]
mod label {
    pub(super) const PENDING_TXS: &str = "newPendingTransactions";
    pub(super) const NEW_HEADS: &str = "newHeads";
    pub(super) const LOGS: &str = "logs";
}

/// State of JSON-RPC websocket subscriptions.
#[derive(Debug)]
pub struct RpcSubscriptions {
    pub connected: Arc<RpcSubscriptionsConnected>,
    pub handles: RpcSubscriptionsHandles,
}

impl RpcSubscriptions {
    /// Creates a new subscriptin manager that automatically spawns all necessary tasks in background.
    pub fn spawn(rx_pending_txs: broadcast::Receiver<Hash>, rx_blocks: broadcast::Receiver<BlockHeader>, rx_logs: broadcast::Receiver<LogMined>) -> Self {
        let connected = Arc::new(RpcSubscriptionsConnected::default());

        Self::spawn_subscriptions_cleaner(Arc::clone(&connected));
        let handles = RpcSubscriptionsHandles {
            new_pending_txs: Self::spawn_new_pending_txs_notifier(Arc::clone(&connected), rx_pending_txs),
            new_heads: Self::spawn_new_heads_notifier(Arc::clone(&connected), rx_blocks),
            logs: Self::spawn_logs_notifier(Arc::clone(&connected), rx_logs),
        };

        Self { connected, handles }
    }

    /// Spawns a new task to clean up closed subscriptions from time to time.
    fn spawn_subscriptions_cleaner(subs: Arc<RpcSubscriptionsConnected>) -> JoinHandle<anyhow::Result<()>> {
        const TASK_NAME: &str = "rpc::sub::cleaner";
        named_spawn(TASK_NAME, async move {
            loop {
                if GlobalState::warn_if_shutdown(TASK_NAME) {
                    return Ok(());
                }

                // remove closed subscriptions
                subs.new_pending_txs.write().await.retain(|_, sub| not(sub.is_closed()));
                subs.new_heads.write().await.retain(|_, sub| not(sub.is_closed()));
                subs.logs.write().await.retain(|_, (sub, _)| not(sub.is_closed()));

                // update metrics
                #[cfg(feature = "metrics")]
                {
                    metrics::set_rpc_subscriptions_active(subs.new_pending_txs.read().await.len() as u64, label::PENDING_TXS);
                    metrics::set_rpc_subscriptions_active(subs.new_heads.read().await.len() as u64, label::NEW_HEADS);
                    metrics::set_rpc_subscriptions_active(subs.logs.read().await.len() as u64, label::LOGS);
                }

                // await next iteration
                traced_sleep(CLEANING_FREQUENCY).await;
            }
        })
    }

    /// Spawns a new task that notifies subscribers about new executed transactions.
    fn spawn_new_pending_txs_notifier(subs: Arc<RpcSubscriptionsConnected>, mut rx_tx_hash: broadcast::Receiver<Hash>) -> JoinHandle<anyhow::Result<()>> {
        const TASK_NAME: &str = "rpc::sub::newPendingTransactions";
        named_spawn(TASK_NAME, async move {
            loop {
                if GlobalState::warn_if_shutdown(TASK_NAME) {
                    return Ok(());
                }

                let Ok(hash) = channel_read!(rx_tx_hash) else {
                    warn_task_tx_closed(TASK_NAME);
                    break;
                };

                let subs = subs.new_pending_txs.read().await;
                Self::notify(subs.values(), hash.to_string()).await;
            }
            Ok(())
        })
    }

    /// Spawns a new task that notifies subscribers about new created blocks.
    fn spawn_new_heads_notifier(subs: Arc<RpcSubscriptionsConnected>, mut rx_block_header: broadcast::Receiver<BlockHeader>) -> JoinHandle<anyhow::Result<()>> {
        const TASK_NAME: &str = "rpc::sub::newHeads";
        named_spawn(TASK_NAME, async move {
            loop {
                if GlobalState::warn_if_shutdown(TASK_NAME) {
                    return Ok(());
                }

                let Ok(header) = channel_read!(rx_block_header) else {
                    warn_task_tx_closed(TASK_NAME);
                    break;
                };

                let subs = subs.new_heads.read().await;
                Self::notify(subs.values(), header).await;
            }
            Ok(())
        })
    }

    /// Spawns a new task that notifies subscribers about new transactions logs.
    fn spawn_logs_notifier(subs: Arc<RpcSubscriptionsConnected>, mut rx_log_mined: broadcast::Receiver<LogMined>) -> JoinHandle<anyhow::Result<()>> {
        const TASK_NAME: &str = "rpc::sub::logs";
        named_spawn(TASK_NAME, async move {
            loop {
                if GlobalState::warn_if_shutdown(TASK_NAME) {
                    return Ok(());
                }

                let Ok(log) = channel_read!(rx_log_mined) else {
                    warn_task_tx_closed(TASK_NAME);
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
        let mut subs = self.new_pending_txs.write().await;
        subs.insert(sink.connection_id(), sink);

        #[cfg(feature = "metrics")]
        metrics::set_rpc_subscriptions_active(subs.len() as u64, label::PENDING_TXS);
    }

    /// Adds a new subscriber to `newHeads` event.
    pub async fn add_new_heads(&self, sink: SubscriptionSink) {
        tracing::debug!(id = %sink.connection_id(), "subscribing to newHeads event");
        let mut subs = self.new_heads.write().await;
        subs.insert(sink.connection_id(), sink);

        #[cfg(feature = "metrics")]
        metrics::set_rpc_subscriptions_active(subs.len() as u64, label::NEW_HEADS);
    }

    /// Adds a new subscriber to `logs` event.
    pub async fn add_logs(&self, sink: SubscriptionSink, filter: LogFilter) {
        tracing::debug!(id = %sink.connection_id(), ?filter, "subscribing to logs event");
        let mut subs = self.logs.write().await;
        subs.insert(sink.connection_id(), (sink, filter));

        #[cfg(feature = "metrics")]
        metrics::set_rpc_subscriptions_active(subs.len() as u64, label::LOGS);
    }
}
