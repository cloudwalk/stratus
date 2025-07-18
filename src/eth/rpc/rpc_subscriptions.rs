use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use futures::join;
use itertools::Itertools;
use jsonrpsee::ConnectionId;
use jsonrpsee::SubscriptionMessage;
use jsonrpsee::SubscriptionSink;
use serde::ser::SerializeMap;
use tokio::sync::RwLock;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio::time::Duration;
use tokio::time::timeout;

use crate::GlobalState;
use crate::eth::primitives::BlockHeader;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogFilterInput;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::RpcError;
use crate::eth::primitives::StratusError;
use crate::eth::primitives::UnixTimeNow;
use crate::eth::rpc::RpcClientApp;
use crate::ext::DisplayExt;
use crate::ext::SleepReason;
use crate::ext::not;
use crate::ext::spawn_named;
use crate::ext::traced_sleep;
use crate::if_else;
#[cfg(feature = "metrics")]
use crate::infra::metrics;
use crate::infra::tracing::warn_task_rx_closed;

/// Frequency of cleaning up closed subscriptions.
const CLEANING_FREQUENCY: Duration = Duration::from_secs(10);

/// Timeout used when sending notifications to subscribers.
const NOTIFICATION_TIMEOUT: Duration = Duration::from_secs(10);

/// Max wait since last checked shutdown in notifier.
const NOTIFIER_SHUTDOWN_CHECK_INTERVAL: Duration = Duration::from_secs(2);

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
    /// Creates a new subscription manager that automatically spawns all necessary tasks in background.
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
        spawn_named(TASK_NAME, async move {
            loop {
                if GlobalState::is_shutdown_warn(TASK_NAME) {
                    return Ok(());
                }

                // store here which subscriptions were cleaned to later log them
                let mut pending_txs_subs_cleaned = Vec::<RpcClientApp>::new();
                let mut new_heads_subs_cleaned = Vec::<RpcClientApp>::new();
                let mut logs_subs_cleaned = Vec::<(RpcClientApp, LogFilterInput)>::new();

                // remove closed subscriptions
                subs.pending_txs.write().await.retain(|_, sub| {
                    let should_keep = not(sub.sink.is_closed());
                    if !should_keep {
                        pending_txs_subs_cleaned.push(sub.client.clone());
                    }
                    should_keep
                });
                subs.new_heads.write().await.retain(|_, sub| {
                    let should_keep = not(sub.sink.is_closed());
                    if !should_keep {
                        new_heads_subs_cleaned.push(sub.client.clone());
                    }
                    should_keep
                });
                subs.logs.write().await.retain(|_, connection_sub_map| {
                    // clear inner map first
                    connection_sub_map.retain(|_, sub| {
                        let should_keep = not(sub.inner.sink.is_closed());
                        if !should_keep {
                            logs_subs_cleaned.push((sub.inner.client.clone(), sub.filter.original_input.clone()));
                        }
                        should_keep
                    });

                    // remove empty connection maps
                    not(connection_sub_map.is_empty())
                });

                // log cleaned subscriptions
                let amount_cleaned = pending_txs_subs_cleaned.len() + new_heads_subs_cleaned.len() + logs_subs_cleaned.len();
                if amount_cleaned > 0 {
                    tracing::info!(
                        amount_cleaned,
                        pending_txs = ?pending_txs_subs_cleaned,
                        new_heads = ?new_heads_subs_cleaned,
                        logs = ?logs_subs_cleaned,
                        "cleaned subscriptions",
                    );
                }

                // update metrics
                #[cfg(feature = "metrics")]
                {
                    // Set cleaned subscriptions gauges to zero, which might be the wrong value
                    // they'll be set back to the correct values in the lines below
                    for client in pending_txs_subs_cleaned {
                        metrics::set_rpc_subscriptions_active(0, label::PENDING_TXS, client.to_string());
                    }
                    for client in new_heads_subs_cleaned {
                        metrics::set_rpc_subscriptions_active(0, label::NEW_HEADS, client.to_string());
                    }
                    for client in logs_subs_cleaned.into_iter().map(|(client, _)| client) {
                        metrics::set_rpc_subscriptions_active(0, label::LOGS, client.to_string());
                    }

                    sub_metrics::update_new_pending_txs_subscription_metrics(&(*subs.pending_txs.read().await));
                    sub_metrics::update_new_heads_subscription_metrics(&(*subs.new_heads.read().await));
                    sub_metrics::update_logs_subscription_metrics(&(*subs.logs.read().await));
                }

                // await next iteration
                traced_sleep(CLEANING_FREQUENCY, SleepReason::Interval).await;
            }
        })
    }

    /// Spawns a new task that notifies subscribers about new executed transactions.
    fn spawn_new_pending_txs_notifier(subs: Arc<RpcSubscriptionsConnected>, mut rx_tx_hash: broadcast::Receiver<Hash>) -> JoinHandle<anyhow::Result<()>> {
        const TASK_NAME: &str = "rpc::sub::newPendingTransactions";
        spawn_named(TASK_NAME, async move {
            loop {
                if GlobalState::is_shutdown_warn(TASK_NAME) {
                    return Ok(());
                }

                let tx_hash = match timeout(NOTIFIER_SHUTDOWN_CHECK_INTERVAL, rx_tx_hash.recv()).await {
                    Ok(Ok(tx)) => tx,
                    Ok(Err(_channel_closed)) => break,
                    Err(_timed_out) => continue,
                };

                let interested_subs = subs.pending_txs.read().await;
                let subscribers = interested_subs.values().collect_vec();

                if !subscribers.is_empty() {
                    // Group clients by type and count how many of each type
                    let mut client_type_counts = std::collections::HashMap::new();

                    for sub in &subscribers {
                        let client_str = sub.client.to_string();
                        let client_type = Self::get_client_type(&client_str);

                        *client_type_counts.entry(client_type).or_insert(0) += 1;
                    }

                    // Format the log message
                    let mut client_summary = Vec::new();
                    for (client_type, count) in client_type_counts.iter() {
                        client_summary.push(format!("{client_type}: {count}"));
                    }

                    tracing::info!(
                        tx_hash = ?tx_hash,
                        clients = ?client_summary.join(", "),
                        "notifying subscribers about new pending transaction"
                    );
                }
                let value = serde_json::value::RawValue::from_string(serde_json::to_string(&tx_hash)?)?;
                Self::notify(subscribers, value);
            }
            warn_task_rx_closed(TASK_NAME);
            Ok(())
        })
    }

    /// Spawns a new task that notifies subscribers about new created blocks.
    fn spawn_new_heads_notifier(subs: Arc<RpcSubscriptionsConnected>, mut rx_block: broadcast::Receiver<BlockHeader>) -> JoinHandle<anyhow::Result<()>> {
        const TASK_NAME: &str = "rpc::sub::newHeads";
        spawn_named(TASK_NAME, async move {
            loop {
                if GlobalState::is_shutdown_warn(TASK_NAME) {
                    return Ok(());
                }

                let block_header = match timeout(NOTIFIER_SHUTDOWN_CHECK_INTERVAL, rx_block.recv()).await {
                    Ok(Ok(block)) => block,
                    Ok(Err(_channel_closed)) => break,
                    Err(_timed_out) => continue,
                };

                let interested_subs = subs.new_heads.read().await;
                let subscribers = interested_subs.values().collect_vec();

                if !subscribers.is_empty() {
                    // Group clients by type and count how many of each type
                    let mut client_type_counts = std::collections::HashMap::new();

                    for sub in &subscribers {
                        let client_str = sub.client.to_string();
                        let client_type = Self::get_client_type(&client_str);

                        *client_type_counts.entry(client_type).or_insert(0) += 1;
                    }

                    // Format the log message
                    let mut client_summary = Vec::new();
                    for (client_type, count) in client_type_counts.iter() {
                        client_summary.push(format!("{client_type}: {count}"));
                    }

                    tracing::info!(
                        block_number = ?block_header.number,
                        block_hash = ?block_header.hash,
                        clients = ?client_summary.join(", "),
                        "notifying subscribers about new block"
                    );
                }

                Self::notify(subscribers, block_header);
            }
            warn_task_rx_closed(TASK_NAME);
            Ok(())
        })
    }

    /// Spawns a new task that notifies subscribers about new transactions logs.
    fn spawn_logs_notifier(subs: Arc<RpcSubscriptionsConnected>, mut rx_log_mined: broadcast::Receiver<LogMined>) -> JoinHandle<anyhow::Result<()>> {
        const TASK_NAME: &str = "rpc::sub::logs";
        spawn_named(TASK_NAME, async move {
            loop {
                if GlobalState::is_shutdown_warn(TASK_NAME) {
                    return Ok(());
                }

                let log = match timeout(NOTIFIER_SHUTDOWN_CHECK_INTERVAL, rx_log_mined.recv()).await {
                    Ok(Ok(log)) => log,
                    Ok(Err(_channel_closed)) => break,
                    Err(_timed_out) => continue,
                };

                let interested_subs = subs.logs.read().await;
                let matching_subscribers = interested_subs
                    .values()
                    .flat_map(HashMap::values)
                    .filter_map(|s| if_else!(s.filter.matches(&log), Some(&s.inner), None))
                    .collect_vec();

                if !matching_subscribers.is_empty() {
                    // Group clients by type and count how many of each type
                    let mut client_type_counts = std::collections::HashMap::new();

                    for sub in &matching_subscribers {
                        let client_str = sub.client.to_string();
                        let client_type = Self::get_client_type(&client_str);

                        *client_type_counts.entry(client_type).or_insert(0) += 1;
                    }

                    // Format the log message
                    let mut client_summary = Vec::new();
                    for (client_type, count) in client_type_counts.iter() {
                        client_summary.push(format!("{client_type}: {count}"));
                    }

                    tracing::info!(
                        log_block_number = ?log.block_number,
                        log_tx_hash = ?log.transaction_hash,
                        clients = ?client_summary.join(", "),
                        "notifying subscribers about new logs"
                    );
                }

                Self::notify(matching_subscribers, log);
            }
            warn_task_rx_closed(TASK_NAME);
            Ok(())
        })
    }

    // -------------------------------------------------------------------------
    // Helpers
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

            // track metric
            sub.inc_sent();

            // send
            let sink = Arc::clone(&sub.sink);
            let msg_clone = msg.clone();
            spawn_named("rpc::sub::notify", async move {
                if let Err(e) = sink.send_timeout(msg_clone, NOTIFICATION_TIMEOUT).await {
                    match e {
                        jsonrpsee::SendTimeoutError::Timeout(msg) => tracing::error!(reason = ?msg, "failed to send subscription notification"),
                        jsonrpsee::SendTimeoutError::Closed(msg) =>
                            tracing::info!(reason = ?msg, "failed to send subscription notification because the connection was closed"),
                    }
                }
            });
        }
    }

    fn get_client_type(client_str: &str) -> String {
        // Split the client string by "::" and take the first part
        if let Some(index) = client_str.find("::") {
            // Extract the type part (before the first "::")
            client_str[..index].to_string()
        } else {
            // If there's no "::", return "other"
            "other".to_string()
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

    pub fn abort(&self) {
        self.new_pending_txs.abort();
        self.new_heads.abort();
        self.logs.abort();
    }
}

// -----------------------------------------------------------------------------
// Connected clients
// -----------------------------------------------------------------------------

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
    /// Checks if the subscription still active.
    fn is_active(&self) -> bool {
        not(self.sink.is_closed())
    }

    /// Increment the number of messages sent to this subscription.
    fn inc_sent(&self) {
        self.sent.fetch_add(1, Ordering::Relaxed);
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

#[derive(Debug, derive_more::Deref, derive_new::new, serde::Serialize)]
pub struct SubscriptionWithFilter {
    #[deref]
    #[serde(flatten)]
    inner: Subscription,

    filter: LogFilter,
}

/// Active client subscriptions.
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

    /// Adds a new subscriber to `newPendingTransactions` event.
    pub async fn add_new_pending_txs_subscription(&self, rpc_client: &RpcClientApp, sink: SubscriptionSink) {
        tracing::info!(
            id = sink.subscription_id().to_string_ext(),
            %rpc_client,
            "subscribing to newPendingTransactions event"
        );
        let mut subs = self.pending_txs.write().await;
        subs.insert(sink.connection_id(), Subscription::new(rpc_client.clone(), sink.into()));

        #[cfg(feature = "metrics")]
        sub_metrics::update_new_pending_txs_subscription_metrics(&subs);
    }

    /// Adds a new subscriber to `newHeads` event.
    pub async fn add_new_heads_subscription(&self, rpc_client: &RpcClientApp, sink: SubscriptionSink) {
        tracing::info!(
            id = sink.subscription_id().to_string_ext(),
            %rpc_client,
            "subscribing to newHeads event"
        );
        let mut subs = self.new_heads.write().await;
        subs.insert(sink.connection_id(), Subscription::new(rpc_client.clone(), sink.into()));

        #[cfg(feature = "metrics")]
        sub_metrics::update_new_heads_subscription_metrics(&subs);
    }

    /// Adds a new subscriber to `logs` event.
    ///
    /// If the same connection is asking to subscribe with the same filter (which is redundant),
    /// the new subscription will overwrite the newest one.
    pub async fn add_logs_subscription(&self, rpc_client: &RpcClientApp, filter: LogFilter, sink: SubscriptionSink) {
        tracing::info!(
            id = sink.subscription_id().to_string_ext(), ?filter,
            %rpc_client,
            "subscribing to logs event"
        );
        let mut subs = self.logs.write().await;
        let filter_to_subscription_map = subs.entry(sink.connection_id()).or_default();

        // Insert the new subscription, if it already existed with the provided filter, overwrite
        // the previous sink with the newest
        let inner = Subscription::new(rpc_client.clone(), sink.into());
        filter_to_subscription_map.insert(filter.clone(), SubscriptionWithFilter::new(inner, filter));

        #[cfg(feature = "metrics")]
        sub_metrics::update_logs_subscription_metrics(&subs);
    }
}

#[cfg(feature = "metrics")]
mod sub_metrics {
    use super::ConnectionId;
    use super::HashMap;
    use super::Itertools;
    use super::LogFilter;
    use super::RpcClientApp;
    use super::Subscription;
    use super::SubscriptionWithFilter;
    use super::label;
    use super::metrics;

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

    fn update_subscription_count<'a, I>(sub_label: &str, sub_client_app_iter: I)
    where
        I: Iterator<Item = &'a Subscription>,
    {
        let client_counts: HashMap<&RpcClientApp, usize> = sub_client_app_iter.map(|sub| &sub.client).counts();

        for (client, count) in client_counts {
            metrics::set_rpc_subscriptions_active(count as u64, sub_label, client.to_string());
        }
    }
}
