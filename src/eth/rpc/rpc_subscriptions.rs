use std::collections::HashMap;
use std::sync::Arc;

use futures::join;
use itertools::Itertools;
use jsonrpsee::ConnectionId;
use jsonrpsee::SubscriptionMessage;
use jsonrpsee::SubscriptionSink;
use tokio::sync::broadcast;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio::time::Duration;

use crate::channel_read;
use crate::eth::primitives::Block;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::TransactionExecution;
use crate::eth::rpc::RpcClientApp;
use crate::ext::not;
use crate::ext::spawn_named;
use crate::ext::traced_sleep;
use crate::ext::DisplayExt;
use crate::ext::SleepReason;
use crate::if_else;
#[cfg(feature = "metrics")]
use crate::infra::metrics;
use crate::infra::tracing::warn_task_tx_closed;
use crate::GlobalState;

/// Frequency of cleaning up closed subscriptions.
const CLEANING_FREQUENCY: Duration = Duration::from_secs(10);

/// Timeout used when sending notifications to subscribers.
const NOTIFICATION_TIMEOUT: Duration = Duration::from_secs(10);

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
    pub fn spawn(
        rx_pending_txs: broadcast::Receiver<TransactionExecution>,
        rx_blocks: broadcast::Receiver<Block>,
        rx_logs: broadcast::Receiver<LogMined>,
    ) -> Self {
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
        spawn_named(TASK_NAME, async move {
            loop {
                if GlobalState::warn_if_shutdown(TASK_NAME) {
                    return Ok(());
                }

                // remove closed subscriptions
                subs.pending_txs.write().await.retain(|_, s| not(s.sink.is_closed()));
                subs.new_heads.write().await.retain(|_, s| not(s.sink.is_closed()));
                subs.logs.write().await.retain(|_, s| not(s.sink.is_closed()));

                // update metrics
                #[cfg(feature = "metrics")]
                {
                    metrics::set_rpc_subscriptions_active(subs.pending_txs.read().await.len() as u64, label::PENDING_TXS);
                    metrics::set_rpc_subscriptions_active(subs.new_heads.read().await.len() as u64, label::NEW_HEADS);
                    metrics::set_rpc_subscriptions_active(subs.logs.read().await.len() as u64, label::LOGS);
                }

                // await next iteration
                traced_sleep(CLEANING_FREQUENCY, SleepReason::Interval).await;
            }
        })
    }

    /// Spawns a new task that notifies subscribers about new executed transactions.
    fn spawn_new_pending_txs_notifier(
        subs: Arc<RpcSubscriptionsConnected>,
        mut rx_tx_hash: broadcast::Receiver<TransactionExecution>,
    ) -> JoinHandle<anyhow::Result<()>> {
        const TASK_NAME: &str = "rpc::sub::newPendingTransactions";
        spawn_named(TASK_NAME, async move {
            loop {
                if GlobalState::warn_if_shutdown(TASK_NAME) {
                    return Ok(());
                }

                let Ok(tx) = channel_read!(rx_tx_hash) else {
                    warn_task_tx_closed(TASK_NAME);
                    break;
                };

                let subs = subs.pending_txs.read().await;
                Self::notify(subs.values().map(|s| Arc::clone(&s.sink)), tx.hash().to_string()).await;
            }
            Ok(())
        })
    }

    /// Spawns a new task that notifies subscribers about new created blocks.
    fn spawn_new_heads_notifier(subs: Arc<RpcSubscriptionsConnected>, mut rx_block: broadcast::Receiver<Block>) -> JoinHandle<anyhow::Result<()>> {
        const TASK_NAME: &str = "rpc::sub::newHeads";
        spawn_named(TASK_NAME, async move {
            loop {
                if GlobalState::warn_if_shutdown(TASK_NAME) {
                    return Ok(());
                }

                let Ok(block) = channel_read!(rx_block) else {
                    warn_task_tx_closed(TASK_NAME);
                    break;
                };

                let subs = subs.new_heads.read().await;
                Self::notify(subs.values().map(|s| Arc::clone(&s.sink)), block.header).await;
            }
            Ok(())
        })
    }

    /// Spawns a new task that notifies subscribers about new transactions logs.
    fn spawn_logs_notifier(subs: Arc<RpcSubscriptionsConnected>, mut rx_log_mined: broadcast::Receiver<LogMined>) -> JoinHandle<anyhow::Result<()>> {
        const TASK_NAME: &str = "rpc::sub::logs";
        spawn_named(TASK_NAME, async move {
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
                    .filter_map(|s| if_else!(s.filter.matches(&log), Some(Arc::clone(&s.sink)), None))
                    .collect_vec();

                Self::notify(interested_subs.into_iter(), log).await;
            }
            Ok(())
        })
    }

    // -------------------------------------------------------------------------
    // Helpers
    // -------------------------------------------------------------------------

    async fn notify(subs: impl ExactSizeIterator<Item = Arc<SubscriptionSink>>, msg: impl Into<SubscriptionMessage>) {
        if subs.len() == 0 {
            return;
        }

        let msg = msg.into();
        for sub in subs {
            if sub.is_closed() {
                continue;
            }
            let msg_clone = msg.clone();
            spawn_named("rpc::sub::notify", async move {
                if let Err(e) = sub.send_timeout(msg_clone, NOTIFICATION_TIMEOUT).await {
                    tracing::error!(reason = ?e, "failed to send subscription notification");
                }
            });
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

#[derive(Debug, derive_new::new)]
pub struct PendingTransactionSubscription {
    pub client: RpcClientApp,
    pub sink: Arc<SubscriptionSink>,
}

#[derive(Debug, derive_new::new)]
pub struct NewHeadsSubscription {
    pub client: RpcClientApp,
    pub sink: Arc<SubscriptionSink>,
}

#[derive(Debug, derive_new::new)]
pub struct LogsSubscription {
    pub client: RpcClientApp,
    pub filter: LogFilter,
    pub sink: Arc<SubscriptionSink>,
}

/// Active client subscriptions.
#[derive(Debug, Default)]
pub struct RpcSubscriptionsConnected {
    pub pending_txs: RwLock<HashMap<ConnectionId, PendingTransactionSubscription>>,
    pub new_heads: RwLock<HashMap<ConnectionId, NewHeadsSubscription>>,
    pub logs: RwLock<HashMap<ConnectionId, LogsSubscription>>,
}

impl RpcSubscriptionsConnected {
    /// Adds a new subscriber to `newPendingTransactions` event.
    pub async fn add_new_pending_txs(&self, rpc_client: RpcClientApp, sink: SubscriptionSink) {
        tracing::debug!(
            id = sink.subscription_id().to_string_ext(),
            %rpc_client,
            "subscribing to newPendingTransactions event"
        );
        let mut subs = self.pending_txs.write().await;
        subs.insert(sink.connection_id(), PendingTransactionSubscription::new(rpc_client, sink.into()));

        #[cfg(feature = "metrics")]
        metrics::set_rpc_subscriptions_active(subs.len() as u64, label::PENDING_TXS);
    }

    /// Adds a new subscriber to `newHeads` event.
    pub async fn add_new_heads(&self, rpc_client: RpcClientApp, sink: SubscriptionSink) {
        tracing::debug!(
            id = sink.subscription_id().to_string_ext(),
            %rpc_client,
            "subscribing to newHeads event"
        );
        let mut subs = self.new_heads.write().await;
        subs.insert(sink.connection_id(), NewHeadsSubscription::new(rpc_client, sink.into()));

        #[cfg(feature = "metrics")]
        metrics::set_rpc_subscriptions_active(subs.len() as u64, label::NEW_HEADS);
    }

    /// Adds a new subscriber to `logs` event.
    pub async fn add_logs(&self, rpc_client: RpcClientApp, filter: LogFilter, sink: SubscriptionSink) {
        tracing::debug!(
            id = sink.subscription_id().to_string_ext(), ?filter,
            %rpc_client,
            "subscribing to logs event"
        );
        let mut subs = self.logs.write().await;
        subs.insert(sink.connection_id(), LogsSubscription::new(rpc_client, filter, sink.into()));

        #[cfg(feature = "metrics")]
        metrics::set_rpc_subscriptions_active(subs.len() as u64, label::LOGS);
    }
}
