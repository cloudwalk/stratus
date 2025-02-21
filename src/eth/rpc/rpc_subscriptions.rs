use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use futures::join;
use itertools::Itertools;
use jsonrpsee::ConnectionId;
use jsonrpsee::SubscriptionMessage;
use jsonrpsee::SubscriptionSink;
use serde::ser::SerializeMap;
use tokio::sync::broadcast;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio::time::Duration;

use crate::eth::primitives::BlockHeader;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogFilterInput;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::RpcError;
use crate::eth::primitives::StratusError;
use crate::eth::primitives::UnixTimeNow;
use crate::eth::rpc::RpcClientApp;
use crate::ext::not;
use crate::ext::spawn_named;
use crate::ext::traced_sleep;
use crate::ext::DisplayExt;
use crate::ext::SleepReason;
#[cfg(feature = "metrics")]
use crate::infra::metrics;
use crate::infra::tracing::warn_task_rx_closed;
use crate::GlobalState;

/// Frequency for cleaning up closed subscriptions.
const CLEANING_FREQUENCY: Duration = Duration::from_secs(10);

/// Timeout for sending notifications to subscribers.
const NOTIFICATION_TIMEOUT: Duration = Duration::from_secs(10);

/// Maximum wait time since last shutdown check in notifier.
const NOTIFIER_SHUTDOWN_CHECK_INTERVAL: Duration = Duration::from_secs(2);

#[cfg(feature = "metrics")]
mod label {
    pub(super) const PENDING_TXS: &str = "newPendingTransactions";
    pub(super) const NEW_HEADS: &str = "newHeads";
    pub(super) const LOGS: &str = "logs";
}

/// JSON-RPC WebSocket subscription state.
#[derive(Debug)]
pub struct RpcSubscriptions {
    pub connected: Arc<RpcSubscriptionsConnected>,
    pub handles: RpcSubscriptionsHandles,
}

impl RpcSubscriptions {
    /// Creates a new subscription manager that automatically spawns all necessary background tasks.
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

    /// Spawns a background task to periodically clean up closed subscriptions.
    fn spawn_subscriptions_cleaner(subs: Arc<RpcSubscriptionsConnected>) -> JoinHandle<anyhow::Result<()>> {
        const TASK_NAME: &str = "rpc::sub::cleaner";
        spawn_named(TASK_NAME, async move {
            loop {
                if GlobalState::is_shutdown_warn(TASK_NAME) {
                    return Ok(());
                }

                // Collect subscriptions that are cleaned for logging purposes.
                let mut pending_txs_cleaned = Vec::<RpcClientApp>::new();
                let mut new_heads_cleaned = Vec::<RpcClientApp>::new();
                let mut logs_cleaned = Vec::<(RpcClientApp, LogFilterInput)>::new();

                // Remove closed subscriptions for pending transactions.
                subs.pending_txs.write().await.retain(|_, sub| {
                    let keep = not(sub.sink.is_closed());
                    if !keep {
                        pending_txs_cleaned.push(sub.client.clone());
                    }
                    keep
                });
                // Remove closed subscriptions for new heads.
                subs.new_heads.write().await.retain(|_, sub| {
                    let keep = not(sub.sink.is_closed());
                    if !keep {
                        new_heads_cleaned.push(sub.client.clone());
                    }
                    keep
                });
                // Remove closed subscriptions for logs.
                subs.logs.write().await.retain(|_, connection_map| {
                    connection_map.retain(|_, sub| {
                        let keep = not(sub.inner.sink.is_closed());
                        if !keep {
                            logs_cleaned.push((sub.inner.client.clone(), sub.filter.original_input.clone()));
                        }
                        keep
                    });
                    // Remove the map if empty.
                    not(connection_map.is_empty())
                });

                // Log the cleaned subscriptions.
                let total_cleaned = pending_txs_cleaned.len() + new_heads_cleaned.len() + logs_cleaned.len();
                if total_cleaned > 0 {
                    tracing::info!(
                        total_cleaned,
                        pending_txs = ?pending_txs_cleaned,
                        new_heads = ?new_heads_cleaned,
                        logs = ?logs_cleaned,
                        "cleaned subscriptions"
                    );
                }

                // Update metrics.
                #[cfg(feature = "metrics")]
                {
                    for client in pending_txs_cleaned {
                        metrics::set_rpc_subscriptions_active(0, label::PENDING_TXS, client.to_string());
                    }
                    for client in new_heads_cleaned {
                        metrics::set_rpc_subscriptions_active(0, label::NEW_HEADS, client.to_string());
                    }
                    for client in logs_cleaned.into_iter().map(|(client, _)| client) {
                        metrics::set_rpc_subscriptions_active(0, label::LOGS, client.to_string());
                    }

                    sub_metrics::update_new_pending_txs_subscription_metrics(&(*subs.pending_txs.read().await));
                    sub_metrics::update_new_heads_subscription_metrics(&(*subs.new_heads.read().await));
                    sub_metrics::update_logs_subscription_metrics(&(*subs.logs.read().await));
                }

                // Wait for the next iteration.
                traced_sleep(CLEANING_FREQUENCY, SleepReason::Interval).await;
            }
        })
    }

    /// Spawns a task to notify subscribers about new pending transactions.
    fn spawn_new_pending_txs_notifier(subs: Arc<RpcSubscriptionsConnected>, mut rx_tx_hash: broadcast::Receiver<Hash>) -> JoinHandle<anyhow::Result<()>> {
        const TASK_NAME: &str = "rpc::sub::newPendingTransactions";
        spawn_named(TASK_NAME, async move {
            loop {
                if GlobalState::is_shutdown_warn(TASK_NAME) {
                    return Ok(());
                }

                let tx_hash = match timeout(NOTIFIER_SHUTDOWN_CHECK_INTERVAL, rx_tx_hash.recv()).await {
                    Ok(Ok(tx)) => tx,
                    Ok(Err(_)) => break,
                    Err(_) => continue,
                };

                let subs_lock = subs.pending_txs.read().await;
                // Collect a short summary (ID and client) for each subscriber.
                let subs_list = subs_lock.values().map(|s| s.short_info()).collect::<Vec<_>>();

                tracing::info!(
                    tx_hash = ?tx_hash,
                    subscribers = ?subs_list,
                    "notifying subscribers about new pending transaction"
                );

                Self::notify(subs_lock.values().collect(), tx_hash.to_string());
            }
            warn_task_rx_closed(TASK_NAME);
            Ok(())
        })
    }

    /// Spawns a task to notify subscribers about new blocks.
    fn spawn_new_heads_notifier(subs: Arc<RpcSubscriptionsConnected>, mut rx_block: broadcast::Receiver<BlockHeader>) -> JoinHandle<anyhow::Result<()>> {
        const TASK_NAME: &str = "rpc::sub::newHeads";
        spawn_named(TASK_NAME, async move {
            loop {
                if GlobalState::is_shutdown_warn(TASK_NAME) {
                    return Ok(());
                }

                let block_header = match timeout(NOTIFIER_SHUTDOWN_CHECK_INTERVAL, rx_block.recv()).await {
                    Ok(Ok(block)) => block,
                    Ok(Err(_)) => break,
                    Err(_) => continue,
                };

                let subs_lock = subs.new_heads.read().await;
                let subs_list = subs_lock.values().map(|s| s.short_info()).collect::<Vec<_>>();

                tracing::info!(
                    block_number = ?block_header.number,
                    block_hash = ?block_header.hash,
                    subscribers = ?subs_list,
                    "notifying subscribers about new block"
                );

                Self::notify(subs_lock.values().collect(), block_header);
            }
            warn_task_rx_closed(TASK_NAME);
            Ok(())
        })
    }

    /// Spawns a task to notify subscribers about new log events.
    fn spawn_logs_notifier(subs: Arc<RpcSubscriptionsConnected>, mut rx_log_mined: broadcast::Receiver<LogMined>) -> JoinHandle<anyhow::Result<()>> {
        const TASK_NAME: &str = "rpc::sub::logs";
        spawn_named(TASK_NAME, async move {
            loop {
                if GlobalState::is_shutdown_warn(TASK_NAME) {
                    return Ok(());
                }

                let log = match timeout(NOTIFIER_SHUTDOWN_CHECK_INTERVAL, rx_log_mined.recv()).await {
                    Ok(Ok(log)) => log,
                    Ok(Err(_)) => break,
                    Err(_) => continue,
                };

                let subs_lock = subs.logs.read().await;
                let filtered_subs: Vec<&SubscriptionWithFilter> = subs_lock.values().flat_map(HashMap::values).filter(|s| s.filter.matches(&log)).collect();

                // For log events, collect a short summary (ID, client, and filter).
                let subs_list = filtered_subs.iter().map(|s| s.short_info()).collect::<Vec<_>>();

                tracing::info!(
                    log_block_number = ?log.block_number,
                    log_tx_hash = ?log.transaction_hash,
                    interested_subs = ?subs_list,
                    "notifying subscribers about new logs"
                );

                // Send notifications.
                Self::notify(filtered_subs.iter().map(|s| &s.inner).collect(), log);
            }
            warn_task_rx_closed(TASK_NAME);
            Ok(())
        })
    }

    // -------------------------------------------------------------------------
    // Helper for sending notifications
    // -------------------------------------------------------------------------
    fn notify<T>(subs: Vec<&Subscription>, msg: T)
    where
        T: TryInto<SubscriptionMessage>,
        T::Error: fmt::Debug,
    {
        if subs.is_empty() {
            return;
        }

        let msg = match msg.try_into() {
            Ok(msg) => msg,
            Err(e) => {
                tracing::error!(parent: None, reason = ?e, "failed to convert message into subscription message");
                return;
            }
        };

        for sub in subs {
            if not(sub.is_active()) {
                continue;
            }

            // Update metric: increment sent count.
            sub.inc_sent();

            // Send the notification.
            let sink = Arc::clone(&sub.sink);
            let msg_clone = msg.clone();
            spawn_named("rpc::sub::notify", async move {
                if let Err(e) = sink.send_timeout(msg_clone, NOTIFICATION_TIMEOUT).await {
                    tracing::error!(reason = ?e, "failed to send subscription notification");
                }
            });
        }
    }
}

/// Handles for background subscription tasks.
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

/// Represents a subscription for a connected client.
#[derive(Debug, derive_new::new)]
pub struct Subscription {
    #[new(default)]
    created_at: UnixTimeNow,
    client: RpcClientApp,
    sink: Arc<SubscriptionSink>,
    #[new(default)]
    sent: AtomicUsize,
}

impl Subscription {
    /// Checks if the subscription is active.
    fn is_active(&self) -> bool {
        not(self.sink.is_closed())
    }

    /// Increments the count of messages sent to this subscription.
    fn inc_sent(&self) {
        self.sent.fetch_add(1, Ordering::Relaxed);
    }

    /// Returns a short summary (ID and client) for logging purposes.
    fn short_info(&self) -> (String, RpcClientApp) {
        (self.sink.subscription_id().to_string_ext(), self.client.clone())
    }
}

impl serde::Serialize for Subscription {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut s = serializer.serialize_map(Some(5))?;
        s.serialize_entry("created_at", &self.created_at)?;
        s.serialize_entry("client", &self.client)?;
        s.serialize_entry("id", &self.sink.subscription_id())?;
        s.serialize_entry("active", &self.is_active())?;
        s.serialize_entry("sent", &self.sent.load(Ordering::Relaxed))?;
        s.end()
    }
}

/// Represents a subscription with an associated log filter.
#[derive(Debug, derive_more::Deref, derive_new::new, serde::Serialize)]
pub struct SubscriptionWithFilter {
    #[deref]
    #[serde(flatten)]
    inner: Subscription,
    filter: LogFilter,
}

impl SubscriptionWithFilter {
    /// Returns a short summary (ID, client, and filter) for logging.
    fn short_info(&self) -> (String, RpcClientApp, &LogFilter) {
        (self.inner.sink.subscription_id().to_string_ext(), self.inner.client.clone(), &self.filter)
    }
}

/// Active subscriptions for connected clients.
#[derive(Debug, Default)]
pub struct RpcSubscriptionsConnected {
    pub pending_txs: RwLock<HashMap<ConnectionId, Subscription>>,
    pub new_heads: RwLock<HashMap<ConnectionId, Subscription>>,
    pub logs: RwLock<HashMap<ConnectionId, HashMap<LogFilter, SubscriptionWithFilter>>>,
}

impl RpcSubscriptionsConnected {
    /// Checks the number of subscriptions for a given client.
    pub async fn check_client_subscriptions(&self, max_subscriptions: u32, client: &RpcClientApp) -> Result<(), StratusError> {
        let pending_txs = self.pending_txs.read().await.values().filter(|s| s.client == *client).count();
        let new_heads = self.new_heads.read().await.values().filter(|s| s.client == *client).count();
        let logs = self
            .logs
            .read()
            .await
            .values()
            .flat_map(HashMap::values)
            .filter(|s| s.client == *client)
            .count();
        tracing::info!(%pending_txs, %new_heads, %logs, "current client subscriptions");

        if pending_txs + new_heads + logs >= max_subscriptions as usize {
            return Err(RpcError::SubscriptionLimit { max: max_subscriptions }.into());
        }

        Ok(())
    }

    /// Adds a new subscriber for the `newPendingTransactions` event.
    pub async fn add_new_pending_txs_subscription(&self, rpc_client: &RpcClientApp, sink: SubscriptionSink) {
        tracing::debug!(
            id = sink.subscription_id().to_string_ext(),
            %rpc_client,
            "subscribing to newPendingTransactions event"
        );
        let mut subs = self.pending_txs.write().await;
        subs.insert(sink.connection_id(), Subscription::new(rpc_client.clone(), sink.into()));

        #[cfg(feature = "metrics")]
        sub_metrics::update_new_pending_txs_subscription_metrics(&subs);
    }

    /// Adds a new subscriber for the `newHeads` event.
    pub async fn add_new_heads_subscription(&self, rpc_client: &RpcClientApp, sink: SubscriptionSink) {
        tracing::debug!(
            id = sink.subscription_id().to_string_ext(),
            %rpc_client,
            "subscribing to newHeads event"
        );
        let mut subs = self.new_heads.write().await;
        subs.insert(sink.connection_id(), Subscription::new(rpc_client.clone(), sink.into()));

        #[cfg(feature = "metrics")]
        sub_metrics::update_new_heads_subscription_metrics(&subs);
    }

    /// Adds a new subscriber for the `logs` event.
    ///
    /// If the same connection subscribes with the same filter, the new subscription overwrites the previous one.
    pub async fn add_logs_subscription(&self, rpc_client: &RpcClientApp, filter: LogFilter, sink: SubscriptionSink) {
        tracing::debug!(
            id = sink.subscription_id().to_string_ext(),
            %rpc_client,
            "subscribing to logs event"
        );
        let mut subs = self.logs.write().await;
        let filter_map = subs.entry(sink.connection_id()).or_default();

        let inner = Subscription::new(rpc_client.clone(), sink.into());
        filter_map.insert(filter.clone(), SubscriptionWithFilter::new(inner, filter));

        #[cfg(feature = "metrics")]
        sub_metrics::update_logs_subscription_metrics(&subs);
    }
}

#[cfg(feature = "metrics")]
mod sub_metrics {
    use super::label;
    use super::metrics;
    use super::ConnectionId;
    use super::HashMap;
    use super::Itertools;
    use super::LogFilter;
    use super::RpcClientApp;
    use super::Subscription;
    use super::SubscriptionWithFilter;

    pub fn update_new_pending_txs_subscription_metrics(subs: &HashMap<ConnectionId, Subscription>) {
        update_subscription_count(label::PENDING_TXS, subs.values());
    }

    pub fn update_new_heads_subscription_metrics(subs: &HashMap<ConnectionId, Subscription>) {
        update_subscription_count(label::NEW_HEADS, subs.values());
    }

    pub fn update_logs_subscription_metrics(subs: &HashMap<ConnectionId, HashMap<LogFilter, SubscriptionWithFilter>>) {
        update_subscription_count(
            label::LOGS,
            subs.values().flat_map(HashMap::values).map(|sub_with_filter| &sub_with_filter.inner),
        );
    }

    fn update_subscription_count<'a, I>(sub_label: &str, sub_iter: I)
    where
        I: Iterator<Item = &'a Subscription>,
    {
        let client_counts: HashMap<&RpcClientApp, usize> = sub_iter.map(|sub| &sub.client).counts();

        for (client, count) in client_counts {
            metrics::set_rpc_subscriptions_active(count as u64, sub_label, client.to_string());
        }
    }
}
