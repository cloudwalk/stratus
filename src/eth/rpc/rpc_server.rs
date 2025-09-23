//! RPC server for HTTP and WS.

use std::collections::HashMap;
use std::ops::Deref;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::Duration;

use alloy_primitives::U256;
use alloy_rpc_types_trace::geth::GethDebugTracingOptions;
use alloy_rpc_types_trace::geth::GethTrace;
use anyhow::Result;
use futures::join;
use http::Method;
use itertools::Itertools;
use jsonrpsee::Extensions;
use jsonrpsee::IntoSubscriptionCloseResponse;
use jsonrpsee::PendingSubscriptionSink;
use jsonrpsee::server::BatchRequestConfig;
use jsonrpsee::server::RandomStringIdProvider;
use jsonrpsee::server::RpcModule;
use jsonrpsee::server::Server as RpcServer;
use jsonrpsee::server::ServerConfig;
use jsonrpsee::server::ServerHandle;
use jsonrpsee::server::middleware::http::ProxyGetRequestLayer;
use jsonrpsee::types::Params;
use jsonrpsee::ws_client::RpcServiceBuilder;
use parking_lot::RwLock;
use serde_json::json;
use tokio::runtime::Handle;
use tokio::select;
use tokio::sync::Semaphore;
use tokio::sync::SemaphorePermit;
use tower_http::cors::Any;
use tower_http::cors::CorsLayer;
use tracing::Instrument;
use tracing::Span;
use tracing::field;
use tracing::info_span;

use crate::GlobalState;
use crate::NodeMode;
use crate::alias::AlloyReceipt;
use crate::alias::JsonValue;
use crate::config::StratusConfig;
use crate::eth::codegen;
use crate::eth::codegen::CONTRACTS;
use crate::eth::decode;
use crate::eth::executor::Executor;
use crate::eth::follower::consensus::Consensus;
use crate::eth::follower::importer::Importer;
use crate::eth::follower::importer::ImporterConfig;
use crate::eth::miner::Miner;
use crate::eth::miner::MinerMode;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::CallInput;
use crate::eth::primitives::ChainId;
use crate::eth::primitives::ConsensusError;
use crate::eth::primitives::DecodeInputError;
use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::Hash;
use crate::eth::primitives::ImporterError;
use crate::eth::primitives::LogFilterInput;
#[cfg(feature = "dev")]
use crate::eth::primitives::Nonce;
use crate::eth::primitives::PointInTime;
use crate::eth::primitives::RpcError;
use crate::eth::primitives::SlotIndex;
#[cfg(feature = "dev")]
use crate::eth::primitives::SlotValue;
use crate::eth::primitives::StateError;
use crate::eth::primitives::StorageError;
use crate::eth::primitives::StratusError;
use crate::eth::primitives::TransactionError;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::TransactionStage;
#[cfg(feature = "dev")]
use crate::eth::primitives::Wei;
use crate::eth::rpc::RpcContext;
use crate::eth::rpc::RpcHttpMiddleware;
use crate::eth::rpc::RpcMiddleware;
use crate::eth::rpc::RpcServerConfig;
use crate::eth::rpc::RpcSubscriptions;
use crate::eth::rpc::next_rpc_param;
use crate::eth::rpc::next_rpc_param_or_default;
use crate::eth::rpc::rpc_parser::RpcExtensionsExt;
use crate::eth::rpc::rpc_subscriptions::RpcSubscriptionsHandles;
use crate::eth::storage::ReadKind;
use crate::eth::storage::StratusStorage;
use crate::ext::InfallibleExt;
use crate::ext::WatchReceiverExt;
use crate::ext::not;
use crate::ext::parse_duration;
use crate::ext::to_json_string;
use crate::ext::to_json_value;
use crate::infra::build_info;
use crate::infra::metrics;
use crate::infra::tracing::SpanExt;
use crate::log_and_err;
// -----------------------------------------------------------------------------
// Server
// -----------------------------------------------------------------------------

#[derive(Clone, derive_new::new)]
pub struct Server {
    // services
    pub storage: Arc<StratusStorage>,
    pub executor: Arc<Executor>,
    pub miner: Arc<Miner>,
    pub importer: Arc<RwLock<Option<Arc<Importer>>>>,

    // config
    pub app_config: StratusConfig,
    pub rpc_config: RpcServerConfig,
    pub chain_id: ChainId,
}

impl Server {
    pub async fn serve(self) -> Result<()> {
        let this = Arc::new(self);
        this.update_health().await;

        const TASK_NAME: &str = "rpc-server";
        let health_worker_handle = tokio::task::spawn({
            let this = Arc::clone(&this);
            async move {
                this.health_worker().await;
            }
        });
        this.update_health().await;
        let mut health = GlobalState::get_health_receiver();
        health.mark_unchanged();
        let (server_handle, subscriptions) = loop {
            let (server_handle, subscriptions) = this._serve().await?;
            let server_handle_watch = server_handle.clone();

            // await for cancellation or jsonrpsee to stop (should not happen) or health changes
            select! {
                // If the server stop unexpectedly, shutdown stratus and break the loop
                _ = server_handle_watch.stopped() => {
                    GlobalState::shutdown_from(TASK_NAME, "finished unexpectedly");
                    break (server_handle, subscriptions);
                },
                // If a shutdown is requested, stop the server and break the loop
                _ = GlobalState::wait_shutdown_warn(TASK_NAME) => {
                    let _ = server_handle.stop();
                    break (server_handle, subscriptions);
                },
                // If the health state changes to unhealthy, stop the server and subscriptions and recreate them (causing all connections to be dropped)
                _ = health.wait_for_change(|healthy| GlobalState::restart_on_unhealthy() && !healthy) => {
                    tracing::info!("health state changed to unhealthy, restarting the rpc server");
                    let _ = server_handle.stop();
                    subscriptions.abort();
                    join!(server_handle.stopped(), subscriptions.stopped());
                }
            }
        };
        let res = join!(server_handle.stopped(), subscriptions.stopped(), health_worker_handle);
        res.2?;
        Ok(())
    }

    /// Starts JSON-RPC server.
    async fn _serve(&self) -> anyhow::Result<(ServerHandle, RpcSubscriptionsHandles)> {
        let this = self.clone();
        const TASK_NAME: &str = "rpc-server";
        tracing::info!(%this.rpc_config.rpc_address, %this.rpc_config.rpc_max_connections, "creating {}", TASK_NAME);

        // configure subscriptions
        let subs = RpcSubscriptions::spawn(
            this.miner.notifier_pending_txs.subscribe(),
            this.miner.notifier_blocks.subscribe(),
            this.miner.notifier_logs.subscribe(),
        );

        // configure context
        let ctx = RpcContext {
            server: Arc::new(this.clone()),
            client_version: "stratus",
            subs: Arc::clone(&subs.connected),
        };

        // configure module
        let mut module = RpcModule::<RpcContext>::new(ctx);
        module = register_methods(module)?;

        // configure middleware
        let cors = CorsLayer::new().allow_methods([Method::POST]).allow_origin(Any).allow_headers(Any);
        let rpc_middleware = RpcServiceBuilder::new().layer_fn(RpcMiddleware::new);
        let http_middleware = tower::ServiceBuilder::new().layer(cors).layer_fn(RpcHttpMiddleware::new).layer(
            ProxyGetRequestLayer::new([
                ("/health", "stratus_health"),
                ("/version", "stratus_version"),
                ("/config", "stratus_config"),
                ("/state", "stratus_state"),
            ])
            .unwrap(),
        );

        let server_config = ServerConfig::builder()
            .set_id_provider(RandomStringIdProvider::new(8))
            .max_connections(this.rpc_config.rpc_max_connections)
            .max_response_body_size(this.rpc_config.rpc_max_response_size_bytes)
            .set_batch_request_config(BatchRequestConfig::Limit(this.rpc_config.batch_request_limit))
            .build();

        // serve module
        let server = RpcServer::builder()
            .set_rpc_middleware(rpc_middleware)
            .set_http_middleware(http_middleware)
            .set_config(server_config)
            .build(this.rpc_config.rpc_address)
            .await?;

        let handle_rpc_server = server.start(module);
        Ok((handle_rpc_server, subs.handles))
    }

    pub async fn health_worker(&self) {
        // Create an interval that ticks at the configured interval
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(self.rpc_config.health_check_interval_ms));

        // Enter an infinite loop to periodically run the health check
        loop {
            // Call the health function and ignore its result
            self.update_health().await;

            // Wait for the next tick
            interval.tick().await;
            if GlobalState::is_shutdown() {
                break;
            }
        }
    }

    async fn update_health(&self) {
        let is_healthy = self.health().await;
        GlobalState::set_health(is_healthy);
        metrics::set_consensus_is_ready(if is_healthy { 1u64 } else { 0u64 });
    }

    fn read_importer(&self) -> Option<Arc<Importer>> {
        self.importer.read().as_ref().map(Arc::clone)
    }

    pub fn set_importer(&self, importer: Option<Arc<Importer>>) {
        *self.importer.write() = importer;
    }

    async fn health(&self) -> bool {
        match GlobalState::get_node_mode() {
            NodeMode::Leader | NodeMode::FakeLeader => true,
            NodeMode::Follower =>
                if GlobalState::is_importer_shutdown() {
                    tracing::warn!("stratus is unhealthy because importer is shutdown");
                    false
                } else {
                    match self.read_importer() {
                        Some(importer) => importer.should_serve().await,
                        None => {
                            tracing::warn!("stratus is unhealthy because importer is not available");
                            false
                        }
                    }
                },
        }
    }
}

fn register_methods(mut module: RpcModule<RpcContext>) -> anyhow::Result<RpcModule<RpcContext>> {
    // dev mode methods
    #[cfg(feature = "dev")]
    {
        module.register_blocking_method("evm_setNextBlockTimestamp", evm_set_next_block_timestamp)?;
        module.register_blocking_method("evm_mine", evm_mine)?;
        module.register_blocking_method("hardhat_reset", stratus_reset)?;
        module.register_blocking_method("stratus_reset", stratus_reset)?;
        module.register_blocking_method("hardhat_setStorageAt", stratus_set_storage_at)?;
        module.register_blocking_method("stratus_setStorageAt", stratus_set_storage_at)?;
        module.register_blocking_method("hardhat_setNonce", stratus_set_nonce)?;
        module.register_blocking_method("stratus_setNonce", stratus_set_nonce)?;
        module.register_blocking_method("hardhat_setBalance", stratus_set_balance)?;
        module.register_blocking_method("stratus_setBalance", stratus_set_balance)?;
        module.register_blocking_method("hardhat_setCode", stratus_set_code)?;
        module.register_blocking_method("stratus_setCode", stratus_set_code)?;
    }

    // stratus status
    module.register_async_method("stratus_health", stratus_health)?;

    // stratus admin
    module.register_method("stratus_enableTransactions", stratus_enable_transactions)?;
    module.register_method("stratus_disableTransactions", stratus_disable_transactions)?;
    module.register_method("stratus_enableMiner", stratus_enable_miner)?;
    module.register_method("stratus_disableMiner", stratus_disable_miner)?;
    module.register_method("stratus_enableUnknownClients", stratus_enable_unknown_clients)?;
    module.register_method("stratus_disableUnknownClients", stratus_disable_unknown_clients)?;
    module.register_method("stratus_enableRestartOnUnhealthy", stratus_enable_restart_on_unhealthy)?;
    module.register_method("stratus_disableRestartOnUnhealthy", stratus_disable_restart_on_unhealthy)?;
    module.register_async_method("stratus_changeToLeader", stratus_change_to_leader)?;
    module.register_async_method("stratus_changeToFollower", stratus_change_to_follower)?;
    module.register_async_method("stratus_initImporter", stratus_init_importer)?;
    module.register_method("stratus_shutdownImporter", stratus_shutdown_importer)?;
    module.register_async_method("stratus_changeMinerMode", stratus_change_miner_mode)?;

    // stratus state
    module.register_method("stratus_version", stratus_version)?;
    module.register_method("stratus_config", stratus_config)?;
    module.register_method("stratus_state", stratus_state)?;

    module.register_async_method("stratus_getSubscriptions", stratus_get_subscriptions)?;
    module.register_method("stratus_pendingTransactionsCount", stratus_pending_transactions_count)?;

    // blockchain
    module.register_method("net_version", net_version)?;
    module.register_async_method("net_listening", net_listening)?;
    module.register_method("eth_chainId", eth_chain_id)?;
    module.register_method("web3_clientVersion", web3_client_version)?;

    // gas
    module.register_method("eth_gasPrice", eth_gas_price)?;

    // stratus importing helpers
    module.register_blocking_method("stratus_getBlockAndReceipts", stratus_get_block_and_receipts)?;
    module.register_blocking_method("stratus_getBlockWithChanges", stratus_get_block_with_changes)?;

    // block
    module.register_blocking_method("eth_blockNumber", eth_block_number)?;
    module.register_blocking_method("eth_getBlockByNumber", eth_get_block_by_number)?;
    module.register_blocking_method("eth_getBlockByHash", eth_get_block_by_hash)?;
    module.register_method("eth_getUncleByBlockHashAndIndex", eth_get_uncle_by_block_hash_and_index)?;

    // transactions
    module.register_blocking_method("eth_getTransactionByHash", eth_get_transaction_by_hash)?;
    module.register_blocking_method("eth_getTransactionReceipt", eth_get_transaction_receipt)?;
    module.register_blocking_method("eth_estimateGas", eth_estimate_gas)?;
    module.register_blocking_method("eth_call", eth_call)?;
    module.register_blocking_method("eth_sendRawTransaction", eth_send_raw_transaction)?;
    module.register_blocking_method("stratus_call", stratus_call)?;
    module.register_blocking_method("stratus_getTransactionResult", stratus_get_transaction_result)?;
    module.register_blocking_method("debug_traceTransaction", debug_trace_transaction)?;

    // logs
    module.register_blocking_method("eth_getLogs", eth_get_logs)?;

    // account
    module.register_method("eth_accounts", eth_accounts)?;
    module.register_blocking_method("eth_getTransactionCount", eth_get_transaction_count)?;
    module.register_blocking_method("eth_getBalance", eth_get_balance)?;
    module.register_blocking_method("eth_getCode", eth_get_code)?;

    // storage
    module.register_blocking_method("eth_getStorageAt", eth_get_storage_at)?;

    // subscriptions
    module.register_subscription("eth_subscribe", "eth_subscription", "eth_unsubscribe", eth_subscribe)?;

    Ok(module)
}

// -----------------------------------------------------------------------------
// Debug
// -----------------------------------------------------------------------------

#[cfg(feature = "dev")]
fn evm_mine(_params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> Result<JsonValue, StratusError> {
    ctx.server.miner.mine_local_and_commit()?;
    Ok(to_json_value(true))
}

#[cfg(feature = "dev")]
fn evm_set_next_block_timestamp(params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> Result<JsonValue, StratusError> {
    use crate::eth::primitives::UnixTime;
    use crate::log_and_err;

    let (_, timestamp) = next_rpc_param::<UnixTime>(params.sequence())?;
    let latest = ctx.server.storage.read_block(BlockFilter::Latest)?;
    match latest {
        Some(block) => UnixTime::set_offset(block.header.timestamp, timestamp)?,
        None => return log_and_err!("reading latest block returned None")?,
    }
    Ok(to_json_value(timestamp))
}

// -----------------------------------------------------------------------------
// Status - Health checks
// -----------------------------------------------------------------------------

#[allow(unused_variables)]
async fn stratus_health(_: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> Result<JsonValue, StratusError> {
    #[cfg(feature = "dev")]
    ctx.server.update_health().await;

    if GlobalState::is_shutdown() {
        tracing::warn!("liveness check failed because of shutdown");
        return Err(StateError::StratusShutdown.into());
    }

    if GlobalState::is_healthy() {
        Ok(json!(true))
    } else {
        Err(StateError::StratusNotReady.into())
    }
}

// -----------------------------------------------------------------------------
// Stratus - Admin
// -----------------------------------------------------------------------------

#[cfg(feature = "dev")]
fn stratus_reset(_: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> Result<JsonValue, StratusError> {
    ctx.server.storage.reset_to_genesis()?;
    Ok(to_json_value(true))
}

#[cfg(feature = "dev")]
fn stratus_set_storage_at(params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> Result<JsonValue, StratusError> {
    let (params, address) = next_rpc_param::<Address>(params.sequence())?;
    let (params, index) = next_rpc_param::<SlotIndex>(params)?;
    let (_, value) = next_rpc_param::<SlotValue>(params)?;

    tracing::info!(%address, %index, %value, "setting storage at address and index");

    ctx.server.storage.set_storage_at(address, index, value)?;

    Ok(to_json_value(true))
}

#[cfg(feature = "dev")]
fn stratus_set_nonce(params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> Result<JsonValue, StratusError> {
    let (params, address) = next_rpc_param::<Address>(params.sequence())?;
    let (_, nonce) = next_rpc_param::<Nonce>(params)?;

    tracing::info!(%address, %nonce, "setting nonce for address");

    ctx.server.storage.set_nonce(address, nonce)?;

    Ok(to_json_value(true))
}

#[cfg(feature = "dev")]
fn stratus_set_balance(params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> Result<JsonValue, StratusError> {
    let (params, address) = next_rpc_param::<Address>(params.sequence())?;
    let (_, balance) = next_rpc_param::<Wei>(params)?;

    tracing::info!(%address, %balance, "setting balance for address");

    ctx.server.storage.set_balance(address, balance)?;

    Ok(to_json_value(true))
}

#[cfg(feature = "dev")]
fn stratus_set_code(params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> Result<JsonValue, StratusError> {
    let (params, address) = next_rpc_param::<Address>(params.sequence())?;
    let (_, code) = next_rpc_param::<Bytes>(params)?;

    tracing::info!(%address, code_size = %code.0.len(), "setting code for address");

    ctx.server.storage.set_code(address, code)?;

    Ok(to_json_value(true))
}

static MODE_CHANGE_SEMAPHORE: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(1));

async fn stratus_change_to_leader(_: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, StratusError> {
    ext.authentication().auth_admin()?;
    let permit = MODE_CHANGE_SEMAPHORE.try_acquire();
    let _permit: SemaphorePermit = match permit {
        Ok(permit) => permit,
        Err(_) => return Err(StateError::ModeChangeInProgress.into()),
    };

    const LEADER_MINER_INTERVAL: Duration = Duration::from_secs(1);
    tracing::info!("starting process to change node to leader");

    if GlobalState::get_node_mode() == NodeMode::Leader {
        tracing::info!("node is already in leader mode, no changes made");
        return Ok(json!(false));
    }

    if GlobalState::is_transactions_enabled() {
        tracing::error!("transactions are currently enabled, cannot change node mode");
        return Err(StateError::TransactionsEnabled.into());
    }

    tracing::info!("shutting down importer");
    let shutdown_importer_result = stratus_shutdown_importer(Params::new(None), &ctx, &ext);
    match shutdown_importer_result {
        Ok(_) => tracing::info!("importer shutdown successfully"),
        Err(StratusError::Importer(ImporterError::AlreadyShutdown)) => {
            tracing::warn!("importer is already shutdown, continuing");
        }
        Err(e) => {
            tracing::error!(reason = ?e, "failed to shutdown importer");
            return Err(e);
        }
    }

    tracing::info!("wait for importer to shutdown");
    GlobalState::wait_for_importer_to_finish().await;

    let change_miner_mode_result = change_miner_mode(MinerMode::Interval(LEADER_MINER_INTERVAL), &ctx).await;
    if let Err(e) = change_miner_mode_result {
        tracing::error!(reason = ?e, "failed to change miner mode");
        return Err(e);
    }
    tracing::info!("miner mode changed to interval(1s) successfully");

    GlobalState::set_node_mode(NodeMode::Leader);
    ctx.server.storage.clear_cache();
    tracing::info!("node mode changed to leader successfully, cache cleared");

    Ok(json!(true))
}

async fn stratus_change_to_follower(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, StratusError> {
    ext.authentication().auth_admin()?;
    let permit = MODE_CHANGE_SEMAPHORE.try_acquire();
    let _permit: SemaphorePermit = match permit {
        Ok(permit) => permit,
        Err(_) => return Err(StateError::ModeChangeInProgress.into()),
    };

    tracing::info!("starting process to change node to follower");

    if GlobalState::get_node_mode() == NodeMode::Follower {
        tracing::info!("node is already in follower mode, no changes made");
        return Ok(json!(false));
    }

    if GlobalState::is_transactions_enabled() {
        tracing::error!("transactions are currently enabled, cannot change node mode");
        return Err(StateError::TransactionsEnabled.into());
    }

    let pending_txs = ctx.server.storage.pending_transactions();
    if not(pending_txs.is_empty()) {
        tracing::error!(pending_txs = ?pending_txs.len(), "cannot change to follower mode with pending transactions");
        return Err(StorageError::PendingTransactionsExist {
            pending_txs: pending_txs.len(),
        }
        .into());
    }

    let change_miner_mode_result = change_miner_mode(MinerMode::External, &ctx).await;
    if let Err(e) = change_miner_mode_result {
        tracing::error!(reason = ?e, "failed to change miner mode");
        return Err(e);
    }
    tracing::info!("miner mode changed to external successfully");

    GlobalState::set_node_mode(NodeMode::Follower);
    ctx.server.storage.clear_cache();
    tracing::info!("storage cache cleared");

    tracing::info!("initializing importer");
    let init_importer_result = stratus_init_importer(params, Arc::clone(&ctx), ext).await;
    match init_importer_result {
        Ok(_) => tracing::info!("importer initialized successfully"),
        Err(StratusError::Importer(ImporterError::AlreadyRunning)) => {
            tracing::warn!("importer is already running, continuing");
        }
        Err(e) => {
            tracing::error!(reason = ?e, "failed to initialize importer, reverting node mode to leader");
            GlobalState::set_node_mode(NodeMode::Leader);
            return Err(e);
        }
    }
    tracing::info!("node mode changed to follower successfully");

    Ok(json!(true))
}

async fn stratus_init_importer(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, StratusError> {
    ext.authentication().auth_admin()?;
    let (params, external_rpc) = next_rpc_param::<String>(params.sequence())?;
    let (params, external_rpc_ws) = next_rpc_param::<String>(params)?;
    let (params, raw_external_rpc_timeout) = next_rpc_param::<String>(params)?;
    let (params, raw_sync_interval) = next_rpc_param::<String>(params)?;
    let (_, raw_external_rpc_max_response_size_bytes) = next_rpc_param::<String>(params)?;

    let external_rpc_timeout = parse_duration(&raw_external_rpc_timeout).map_err(|e| {
        tracing::error!(reason = ?e, "failed to parse external_rpc_timeout");
        ImporterError::ConfigParseError
    })?;

    let sync_interval = parse_duration(&raw_sync_interval).map_err(|e| {
        tracing::error!(reason = ?e, "failed to parse sync_interval");
        ImporterError::ConfigParseError
    })?;

    let external_rpc_max_response_size_bytes = raw_external_rpc_max_response_size_bytes.parse::<u32>().map_err(|e| {
        tracing::error!(reason = ?e, "failed to parse external_rpc_max_response_size_bytes");
        ImporterError::ConfigParseError
    })?;

    let importer_config = ImporterConfig {
        external_rpc,
        external_rpc_ws: Some(external_rpc_ws),
        external_rpc_timeout,
        sync_interval,
        external_rpc_max_response_size_bytes,
        enable_block_changes_replication: std::env::var("ENABLE_BLOCK_CHANGES_REPLICATION")
            .ok()
            .is_some_and(|val| val == "1" || val == "true"),
    };

    importer_config.init_follower_importer(ctx).await
}

fn stratus_shutdown_importer(_: Params<'_>, ctx: &RpcContext, ext: &Extensions) -> Result<JsonValue, StratusError> {
    ext.authentication().auth_admin()?;
    if GlobalState::get_node_mode() != NodeMode::Follower {
        tracing::error!("node is currently not a follower");
        return Err(StateError::StratusNotFollower.into());
    }

    if GlobalState::is_importer_shutdown() && ctx.server.read_importer().is_none() {
        tracing::error!("importer is already shut down");
        return Err(ImporterError::AlreadyShutdown.into());
    }

    ctx.server.set_importer(None);

    const TASK_NAME: &str = "rpc-server::importer-shutdown";
    GlobalState::shutdown_importer_from(TASK_NAME, "received importer shutdown request");

    Ok(json!(true))
}

async fn stratus_change_miner_mode(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, StratusError> {
    ext.authentication().auth_admin()?;
    let (_, mode_str) = next_rpc_param::<String>(params.sequence())?;

    let mode = MinerMode::from_str(&mode_str).map_err(|e| {
        tracing::error!(reason = ?e, "failed to parse miner mode");
        RpcError::MinerModeParamInvalid
    })?;

    change_miner_mode(mode, &ctx).await
}

/// Tries changing miner mode, returns `Ok(true)` if changed, and `Ok(false)` if no changing was necessary.
///
/// This function also enables the miner after changing it.
async fn change_miner_mode(new_mode: MinerMode, ctx: &RpcContext) -> Result<JsonValue, StratusError> {
    if GlobalState::is_transactions_enabled() {
        tracing::error!("cannot change miner mode while transactions are enabled");
        return Err(StateError::TransactionsEnabled.into());
    }

    let previous_mode = ctx.server.miner.mode();

    if previous_mode == new_mode {
        tracing::warn!(?new_mode, current = ?new_mode, "miner mode already set, skipping");
        return Ok(json!(false));
    }

    if not(ctx.server.miner.is_paused()) && previous_mode.is_interval() {
        return log_and_err!("can't change miner mode from Interval without pausing it first").map_err(Into::into);
    }

    match new_mode {
        MinerMode::External => {
            tracing::info!("changing miner mode to External");

            let pending_txs = ctx.server.storage.pending_transactions();
            if not(pending_txs.is_empty()) {
                tracing::error!(pending_txs = ?pending_txs.len(), "cannot change miner mode to External with pending transactions");
                return Err(StorageError::PendingTransactionsExist {
                    pending_txs: pending_txs.len(),
                }
                .into());
            }

            ctx.server.miner.switch_to_external_mode().await;
        }
        MinerMode::Interval(duration) => {
            tracing::info!(duration = ?duration, "changing miner mode to Interval");

            if ctx.server.read_importer().is_some() {
                tracing::error!("cannot change miner mode to Interval with consensus set");
                return Err(ConsensusError::Set.into());
            }

            ctx.server.miner.start_interval_mining(duration).await;
        }
        MinerMode::Automine => {
            return log_and_err!("Miner mode change to 'automine' is unsupported.").map_err(Into::into);
        }
    }

    Ok(json!(true))
}

fn stratus_enable_unknown_clients(_: Params<'_>, _: &RpcContext, ext: &Extensions) -> Result<bool, StratusError> {
    ext.authentication().auth_admin()?;
    GlobalState::set_unknown_client_enabled(true);
    Ok(GlobalState::is_unknown_client_enabled())
}

fn stratus_disable_unknown_clients(_: Params<'_>, _: &RpcContext, ext: &Extensions) -> Result<bool, StratusError> {
    ext.authentication().auth_admin()?;
    GlobalState::set_unknown_client_enabled(false);
    Ok(GlobalState::is_unknown_client_enabled())
}

fn stratus_enable_transactions(_: Params<'_>, _: &RpcContext, ext: &Extensions) -> Result<bool, StratusError> {
    ext.authentication().auth_admin()?;
    GlobalState::set_transactions_enabled(true);
    Ok(GlobalState::is_transactions_enabled())
}

fn stratus_disable_transactions(_: Params<'_>, _: &RpcContext, ext: &Extensions) -> Result<bool, StratusError> {
    ext.authentication().auth_admin()?;
    GlobalState::set_transactions_enabled(false);
    Ok(GlobalState::is_transactions_enabled())
}

fn stratus_enable_miner(_: Params<'_>, ctx: &RpcContext, ext: &Extensions) -> Result<bool, StratusError> {
    ext.authentication().auth_admin()?;
    ctx.server.miner.unpause();
    Ok(true)
}

fn stratus_disable_miner(_: Params<'_>, ctx: &RpcContext, ext: &Extensions) -> Result<bool, StratusError> {
    ext.authentication().auth_admin()?;
    ctx.server.miner.pause();
    Ok(false)
}

/// Returns the count of executed transactions waiting to enter the next block.
fn stratus_pending_transactions_count(_: Params<'_>, ctx: &RpcContext, _: &Extensions) -> usize {
    ctx.server.storage.pending_transactions().len()
}

fn stratus_disable_restart_on_unhealthy(_: Params<'_>, _: &RpcContext, ext: &Extensions) -> Result<bool, StratusError> {
    ext.authentication().auth_admin()?;
    GlobalState::set_restart_on_unhealthy(false);
    Ok(GlobalState::restart_on_unhealthy())
}

fn stratus_enable_restart_on_unhealthy(_: Params<'_>, _: &RpcContext, ext: &Extensions) -> Result<bool, StratusError> {
    ext.authentication().auth_admin()?;
    GlobalState::set_restart_on_unhealthy(true);
    Ok(GlobalState::restart_on_unhealthy())
}

// -----------------------------------------------------------------------------
// Stratus - State
// -----------------------------------------------------------------------------

fn stratus_version(_: Params<'_>, _: &RpcContext, _: &Extensions) -> Result<JsonValue, StratusError> {
    Ok(build_info::as_json())
}

fn stratus_config(_: Params<'_>, ctx: &RpcContext, ext: &Extensions) -> Result<StratusConfig, StratusError> {
    ext.authentication().auth_admin()?;
    Ok(ctx.server.app_config.clone())
}

fn stratus_state(_: Params<'_>, ctx: &RpcContext, _: &Extensions) -> Result<JsonValue, StratusError> {
    Ok(GlobalState::get_global_state_as_json(ctx))
}

async fn stratus_get_subscriptions(_: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> Result<JsonValue, StratusError> {
    // NOTE: this is a workaround for holding only one lock at a time
    let pending_txs = serde_json::to_value(ctx.subs.pending_txs.read().await.values().collect_vec()).expect_infallible();
    let new_heads = serde_json::to_value(ctx.subs.new_heads.read().await.values().collect_vec()).expect_infallible();
    let logs = serde_json::to_value(ctx.subs.logs.read().await.values().flat_map(HashMap::values).collect_vec()).expect_infallible();

    let response = json!({
        "newPendingTransactions": pending_txs,
        "newHeads": new_heads,
        "logs": logs,
    });
    Ok(response)
}

// -----------------------------------------------------------------------------
// Blockchain
// -----------------------------------------------------------------------------

async fn net_listening(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, StratusError> {
    let net_listening = stratus_health(params, ctx, ext).await;

    tracing::info!(net_listening = ?net_listening, "network listening status");

    net_listening
}

fn net_version(_: Params<'_>, ctx: &RpcContext, _: &Extensions) -> String {
    ctx.server.chain_id.to_string()
}

fn eth_chain_id(_: Params<'_>, ctx: &RpcContext, _: &Extensions) -> String {
    hex_num(ctx.server.chain_id)
}

fn web3_client_version(_: Params<'_>, ctx: &RpcContext, _: &Extensions) -> String {
    ctx.client_version.to_owned()
}

// -----------------------------------------------------------------------------
// Gas
// -----------------------------------------------------------------------------

fn eth_gas_price(_: Params<'_>, _: &RpcContext, _: &Extensions) -> String {
    hex_zero()
}

// -----------------------------------------------------------------------------
// Block
// -----------------------------------------------------------------------------

fn eth_block_number(_params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, StratusError> {
    // enter span
    let _middleware_enter = ext.enter_middleware_span();
    let _method_enter = info_span!("rpc::eth_blockNumber", block_number = field::Empty).entered();

    // execute
    let block_number = ctx.server.storage.read_mined_block_number();
    Span::with(|s| s.rec_str("block_number", &block_number));

    Ok(to_json_value(block_number))
}

fn stratus_get_block_and_receipts(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, StratusError> {
    // enter span
    let _middleware_enter = ext.enter_middleware_span();
    let _method_enter = info_span!("rpc::stratus_getBlockAndReceipts").entered();

    // parse params
    let (_, filter) = next_rpc_param::<BlockFilter>(params.sequence())?;

    // track
    tracing::info!(%filter, "reading block and receipts");

    let Some(block) = ctx.server.storage.read_block(filter)? else {
        tracing::info!(%filter, "block not found");
        return Ok(JsonValue::Null);
    };

    tracing::info!(%filter, "block with transactions found");
    let receipts = block.transactions.iter().cloned().map(AlloyReceipt::from).collect::<Vec<_>>();

    Ok(json!({
        "block": block.to_json_rpc_with_full_transactions(),
        "receipts": receipts,
    }))
}

fn stratus_get_block_with_changes(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, StratusError> {
    // enter span
    let _middleware_enter = ext.enter_middleware_span();
    let _method_enter = info_span!("rpc::stratus_getBlockWithChanges").entered();

    // parse params
    let (_, filter) = next_rpc_param::<BlockFilter>(params.sequence())?;

    // track
    tracing::info!(%filter, "reading block and receipts");

    let Some(block) = ctx.server.storage.read_block_with_changes(filter)? else {
        tracing::info!(%filter, "block not found");
        return Ok(JsonValue::Null);
    };

    tracing::info!(%filter, "block with transactions found");

    Ok(json!(block))
}

fn eth_get_block_by_hash(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, StratusError> {
    eth_get_block_by_selector::<'h'>(params, ctx, ext)
}

fn eth_get_block_by_number(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, StratusError> {
    eth_get_block_by_selector::<'n'>(params, ctx, ext)
}

#[inline(always)]
fn eth_get_block_by_selector<const KIND: char>(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, StratusError> {
    // enter span
    let _middleware_enter = ext.enter_middleware_span();
    let _method_enter = if KIND == 'h' {
        info_span!(
            "rpc::eth_getBlockByHash",
            filter = field::Empty,
            block_number = field::Empty,
            found = field::Empty
        )
        .entered()
    } else {
        info_span!(
            "rpc::eth_getBlockByNumber",
            filter = field::Empty,
            block_number = field::Empty,
            found = field::Empty
        )
        .entered()
    };

    // parse params
    let (params, filter) = next_rpc_param::<BlockFilter>(params.sequence())?;
    let (_, full_transactions) = next_rpc_param::<bool>(params)?;

    // track
    Span::with(|s| s.rec_str("filter", &filter));
    tracing::info!(%filter, %full_transactions, "reading block");

    // execute
    let block = ctx.server.storage.read_block(filter)?;
    Span::with(|s| {
        s.record("found", block.is_some());
        if let Some(ref block) = block {
            s.rec_str("block_number", &block.number());
        }
    });
    match (block, full_transactions) {
        (Some(block), true) => {
            tracing::info!(%filter, "block with full transactions found");
            Ok(block.to_json_rpc_with_full_transactions())
        }
        (Some(block), false) => {
            tracing::info!(%filter, "block with only hashes found");
            Ok(block.to_json_rpc_with_transactions_hashes())
        }
        (None, _) => {
            tracing::info!(%filter, "block not found");
            Ok(JsonValue::Null)
        }
    }
}

fn eth_get_uncle_by_block_hash_and_index(_: Params<'_>, _: &RpcContext, _: &Extensions) -> Result<JsonValue, StratusError> {
    Ok(JsonValue::Null)
}

// -----------------------------------------------------------------------------
// Transaction
// -----------------------------------------------------------------------------

fn eth_get_transaction_by_hash(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, StratusError> {
    // enter span
    let _middleware_enter = ext.enter_middleware_span();
    let _method_enter = info_span!("rpc::eth_getTransactionByHash", tx_hash = field::Empty, found = field::Empty).entered();

    // parse params
    let (_, tx_hash) = next_rpc_param::<Hash>(params.sequence())?;

    // track
    Span::with(|s| s.rec_str("tx_hash", &tx_hash));
    tracing::info!(%tx_hash, "reading transaction");

    // execute
    let tx = ctx.server.storage.read_transaction(tx_hash)?;
    Span::with(|s| {
        s.record("found", tx.is_some());
    });

    match tx {
        Some(tx) => {
            tracing::info!(%tx_hash, "transaction found");
            Ok(tx.to_json_rpc_transaction())
        }
        None => {
            tracing::info!(%tx_hash, "transaction not found");
            Ok(JsonValue::Null)
        }
    }
}

fn rpc_get_transaction_receipt(params: Params<'_>, ctx: Arc<RpcContext>) -> Result<Option<TransactionStage>, StratusError> {
    // parse params
    let (_, tx_hash) = next_rpc_param::<Hash>(params.sequence())?;

    // track
    Span::with(|s| s.rec_str("tx_hash", &tx_hash));
    tracing::info!("reading transaction receipt");

    // execute
    let tx = ctx.server.storage.read_transaction(tx_hash)?;
    Span::with(|s| {
        s.record("found", tx.is_some());
    });

    Ok(tx)
}

fn eth_get_transaction_receipt(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, StratusError> {
    // enter span
    let _middleware_enter = ext.enter_middleware_span();
    let _method_enter = info_span!("rpc::eth_getTransactionReceipt", tx_hash = field::Empty, found = field::Empty).entered();

    match rpc_get_transaction_receipt(params, ctx)? {
        Some(tx) => {
            tracing::info!("transaction receipt found");
            Ok(tx.to_json_rpc_receipt())
        }
        None => {
            tracing::info!("transaction receipt not found");
            Ok(JsonValue::Null)
        }
    }
}

fn stratus_get_transaction_result(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, StratusError> {
    // enter span
    let _middleware_enter = ext.enter_middleware_span();
    let _method_enter = info_span!("rpc::stratus_get_transaction_result", tx_hash = field::Empty, found = field::Empty).entered();

    match rpc_get_transaction_receipt(params, ctx)? {
        Some(tx) => {
            tracing::info!("transaction receipt found");
            Ok(to_json_value(tx.result()))
        }
        None => {
            tracing::info!("transaction receipt not found");
            Ok(JsonValue::Null)
        }
    }
}

fn eth_estimate_gas(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<String, StratusError> {
    // enter span
    let _middleware_enter = ext.enter_middleware_span();
    let _method_enter = info_span!("rpc::eth_estimateGas", tx_from = field::Empty, tx_to = field::Empty).entered();

    // parse params
    let (_, call) = next_rpc_param::<CallInput>(params.sequence())?;

    // track
    Span::with(|s| {
        s.rec_opt("tx_from", &call.from);
        s.rec_opt("tx_to", &call.to);
    });
    tracing::info!("executing eth_estimateGas");

    // execute
    match ctx.server.executor.execute_local_call(call, PointInTime::Mined) {
        // result is success
        Ok(result) if result.is_success() => {
            tracing::info!(tx_output = %result.output, "executed eth_estimateGas with success");
            let overestimated_gas = (result.gas.as_u64()) as f64 * 1.1;
            Ok(hex_num(U256::from(overestimated_gas as u64)))
        }

        // result is failure
        Ok(result) => {
            tracing::warn!(tx_output = %result.output, "executed eth_estimateGas with failure");
            Err(TransactionError::RevertedCall { output: result.output }.into())
        }

        // internal error
        Err(e) => {
            tracing::warn!(reason = ?e, "failed to execute eth_estimateGas");
            Err(e)
        }
    }
}

fn rpc_call(params: Params<'_>, ctx: Arc<RpcContext>) -> Result<EvmExecution, StratusError> {
    // parse params
    let (params, call) = next_rpc_param::<CallInput>(params.sequence())?;
    let (_, filter) = next_rpc_param_or_default::<BlockFilter>(params)?;

    // track
    Span::with(|s| {
        s.rec_opt("tx_from", &call.from);
        s.rec_opt("tx_to", &call.to);
        s.rec_str("filter", &filter);
    });
    tracing::info!(%filter, "executing eth_call");

    // execute
    let point_in_time = ctx.server.storage.translate_to_point_in_time(filter)?;
    ctx.server.executor.execute_local_call(call, point_in_time)
}

fn eth_call(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<String, StratusError> {
    // enter span
    let _middleware_enter = ext.enter_middleware_span();
    let _method_enter = info_span!("rpc::eth_call", tx_from = field::Empty, tx_to = field::Empty, filter = field::Empty).entered();

    match rpc_call(params, ctx) {
        // result is success
        Ok(result) if result.is_success() => {
            tracing::info!(tx_output = %result.output, "executed eth_call with success");
            Ok(hex_data(result.output))
        }
        // result is failure
        Ok(result) => {
            tracing::warn!(tx_output = %result.output, "executed eth_call with failure");
            Err(TransactionError::RevertedCall { output: result.output }.into())
        }
        // internal error
        Err(e) => {
            tracing::warn!(reason = ?e, "failed to execute eth_call");
            Err(e)
        }
    }
}

fn debug_trace_transaction(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, StratusError> {
    // enter span
    let _middleware_enter = ext.enter_middleware_span();
    let _method_enter = info_span!("rpc::debug_traceTransaction", tx_hash = field::Empty,).entered();

    let (params, tx_hash) = next_rpc_param::<Hash>(params.sequence())?;
    let (_, opts) = next_rpc_param_or_default::<Option<GethDebugTracingOptions>>(params)?;
    let trace_unsuccessful_only = ctx
        .server
        .rpc_config
        .rpc_debug_trace_unsuccessful_only
        .as_ref()
        .is_some_and(|inner| inner.contains(ext.rpc_client()));

    match ctx.server.executor.trace_transaction(tx_hash, opts, trace_unsuccessful_only) {
        Ok(result) => {
            tracing::info!(?tx_hash, "executed debug_traceTransaction successfully");

            // Enhance GethTrace with decoded information using serialization approach
            let enhanced_response = enhance_trace_with_decoded_info(&result);

            Ok(enhanced_response)
        }
        Err(err) => {
            tracing::warn!(?err, "error executing debug_traceTransaction");
            Err(err)
        }
    }
}

fn stratus_call(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<String, StratusError> {
    // enter span
    let _middleware_enter = ext.enter_middleware_span();
    let _method_enter = info_span!("rpc::stratus_call", tx_from = field::Empty, tx_to = field::Empty, filter = field::Empty).entered();

    match rpc_call(params, ctx) {
        // result is success
        Ok(result) if result.is_success() => {
            tracing::info!(tx_output = %result.output, "executed stratus_call with success");
            Ok(hex_data(result.output))
        }
        // result is failure
        Ok(result) => {
            tracing::warn!(tx_output = %result.output, "executed stratus_call with failure");
            Err(TransactionError::RevertedCallWithReason {
                reason: (&result.output).into(),
            }
            .into())
        }
        // internal error
        Err(e) => {
            tracing::warn!(reason = ?e, "failed to execute stratus_call");
            Err(e)
        }
    }
}

fn eth_send_raw_transaction(_: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<String, StratusError> {
    // enter span
    let _middleware_enter = ext.enter_middleware_span();
    let _method_enter = info_span!(
        "rpc::eth_sendRawTransaction",
        tx_hash = field::Empty,
        tx_from = field::Empty,
        tx_to = field::Empty,
        tx_nonce = field::Empty
    )
    .entered();

    // get the pre-decoded transaction from extensions
    let (tx, tx_data) = match (ext.get::<TransactionInput>(), ext.get::<Bytes>()) {
        (Some(tx), Some(data)) => (tx.clone(), data.clone()),
        _ => {
            tracing::error!("failed to execute eth_sendRawTransaction because transaction input is not available");
            return Err(RpcError::TransactionInvalid {
                decode_error: "transaction input is not available".to_string(),
            }
            .into());
        }
    };
    let tx_hash = tx.hash;

    // track
    Span::with(|s| {
        s.rec_str("tx_hash", &tx_hash);
        s.rec_str("tx_from", &tx.signer);
        s.rec_opt("tx_to", &tx.to);
        s.rec_str("tx_nonce", &tx.nonce);
    });

    if not(GlobalState::is_transactions_enabled()) {
        tracing::warn!(%tx_hash, "failed to execute eth_sendRawTransaction because transactions are disabled");
        return Err(StateError::TransactionsDisabled.into());
    }

    // execute locally or forward to leader
    match GlobalState::get_node_mode() {
        NodeMode::Leader | NodeMode::FakeLeader => match ctx.server.executor.execute_local_transaction(tx) {
            Ok(_) => Ok(hex_data(tx_hash)),
            Err(e) => {
                tracing::warn!(reason = ?e, ?tx_hash, "failed to execute eth_sendRawTransaction");
                Err(e)
            }
        },
        NodeMode::Follower => match &ctx.server.read_importer() {
            Some(importer) => match Handle::current().block_on(importer.forward_to_leader(tx_hash, tx_data, ext.rpc_client())) {
                Ok(hash) => Ok(hex_data(hash)),
                Err(e) => Err(e),
            },
            None => {
                tracing::error!("unable to forward transaction because consensus is temporarily unavailable for follower node");
                Err(ConsensusError::Unavailable.into())
            }
        },
    }
}

// -----------------------------------------------------------------------------
// Logs
// -----------------------------------------------------------------------------

fn eth_get_logs(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, StratusError> {
    const MAX_BLOCK_RANGE: u64 = 5_000;

    // enter span
    let _middleware_enter = ext.enter_middleware_span();
    let _method_enter = info_span!(
        "rpc::eth_getLogs",
        filter = field::Empty,
        filter_from = field::Empty,
        filter_to = field::Empty,
        filter_range = field::Empty
    )
    .entered();

    // parse params
    let (_, filter_input) = next_rpc_param_or_default::<LogFilterInput>(params.sequence())?;
    let mut filter = filter_input.parse(&ctx.server.storage)?;

    // for this operation, the filter always need the end block specified to calculate the difference
    let to_block = match filter.to_block {
        Some(block) => block,
        None => {
            let block = ctx.server.storage.read_mined_block_number();
            filter.to_block = Some(block);
            block
        }
    };
    let blocks_in_range = filter.from_block.count_to(to_block);

    // track
    Span::with(|s| {
        s.rec_str("filter", &to_json_string(&filter));
        s.rec_str("filter_from", &filter.from_block);
        s.rec_str("filter_to", &to_block);
        s.rec_str("filter_range", &blocks_in_range);
    });
    tracing::info!(?filter, "reading logs");

    // check range
    if blocks_in_range > MAX_BLOCK_RANGE {
        return Err(RpcError::BlockRangeInvalid {
            actual: blocks_in_range,
            max: MAX_BLOCK_RANGE,
        }
        .into());
    }

    // execute
    let logs = ctx.server.storage.read_logs(&filter)?;
    Ok(JsonValue::Array(logs.into_iter().map(|x| x.to_json_rpc_log()).collect()))
}

// -----------------------------------------------------------------------------
// Account
// -----------------------------------------------------------------------------

fn eth_accounts(_: Params<'_>, _ctx: &RpcContext, _: &Extensions) -> Result<JsonValue, StratusError> {
    Ok(json!([]))
}

fn eth_get_transaction_count(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<String, StratusError> {
    // enter span
    let _middleware_enter = ext.enter_middleware_span();
    let _method_enter = info_span!("rpc::eth_getTransactionCount", address = field::Empty, filter = field::Empty).entered();

    // pare params
    let (params, address) = next_rpc_param::<Address>(params.sequence())?;
    let (_, filter) = next_rpc_param_or_default::<BlockFilter>(params)?;

    // track
    Span::with(|s| {
        s.rec_str("address", &address);
        s.rec_str("address", &filter);
    });
    tracing::info!(%address, %filter, "reading account nonce");

    let point_in_time = ctx.server.storage.translate_to_point_in_time(filter)?;
    let account = ctx.server.storage.read_account(address, point_in_time, ReadKind::RPC)?;
    Ok(hex_num(account.nonce))
}

fn eth_get_balance(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<String, StratusError> {
    // enter span
    let _middleware_enter = ext.enter_middleware_span();
    let _method_enter = info_span!("rpc::eth_getBalance", address = field::Empty, filter = field::Empty).entered();

    // parse params
    let (params, address) = next_rpc_param::<Address>(params.sequence())?;
    let (_, filter) = next_rpc_param_or_default::<BlockFilter>(params)?;

    // track
    Span::with(|s| {
        s.rec_str("address", &address);
        s.rec_str("filter", &filter);
    });
    tracing::info!(%address, %filter, "reading account native balance");

    // execute
    let point_in_time = ctx.server.storage.translate_to_point_in_time(filter)?;
    let account = ctx.server.storage.read_account(address, point_in_time, ReadKind::RPC)?;
    Ok(hex_num(account.balance))
}

fn eth_get_code(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<String, StratusError> {
    // enter span
    let _middleware_enter = ext.enter_middleware_span();
    let _method_enter = info_span!("rpc::eth_getCode", address = field::Empty, filter = field::Empty).entered();

    // parse params
    let (params, address) = next_rpc_param::<Address>(params.sequence())?;
    let (_, filter) = next_rpc_param_or_default::<BlockFilter>(params)?;

    // track
    Span::with(|s| {
        s.rec_str("address", &address);
        s.rec_str("filter", &filter);
    });

    // execute
    let point_in_time = ctx.server.storage.translate_to_point_in_time(filter)?;
    let account = ctx.server.storage.read_account(address, point_in_time, ReadKind::RPC)?;

    Ok(account.bytecode.map(|bytecode| hex_data(bytecode.original_bytes())).unwrap_or_else(hex_null))
}

// -----------------------------------------------------------------------------
// Subscriptions
// -----------------------------------------------------------------------------

async fn eth_subscribe(params: Params<'_>, pending: PendingSubscriptionSink, ctx: Arc<RpcContext>, ext: Extensions) -> impl IntoSubscriptionCloseResponse {
    // `middleware_enter` created to be used as a parent by `method_span`
    let middleware_enter = ext.enter_middleware_span();
    let method_span = info_span!("rpc::eth_subscribe", subscription = field::Empty);
    drop(middleware_enter);

    async move {
        // parse params
        let client = ext.rpc_client();
        let (params, event) = match next_rpc_param::<String>(params.sequence()) {
            Ok((params, event)) => (params, event),
            Err(e) => {
                pending.reject(StratusError::from(e)).await;
                return Ok(());
            }
        };

        // check subscription limits
        if let Err(e) = ctx.subs.check_client_subscriptions(ctx.server.rpc_config.rpc_max_subscriptions, client).await {
            pending.reject(e).await;
            return Ok(());
        }

        // track
        Span::with(|s| s.rec_str("subscription", &event));
        tracing::info!(%event, "subscribing to rpc event");

        // execute
        match event.deref() {
            "newPendingTransactions" => {
                ctx.subs.add_new_pending_txs_subscription(client, pending.accept().await?).await;
            }

            "newHeads" => {
                ctx.subs.add_new_heads_subscription(client, pending.accept().await?).await;
            }

            "logs" => {
                let (_, filter) = next_rpc_param_or_default::<LogFilterInput>(params)?;
                let filter = filter.parse(&ctx.server.storage)?;
                ctx.subs.add_logs_subscription(client, filter, pending.accept().await?).await;
            }

            // unsupported
            event => {
                pending
                    .reject(StratusError::from(RpcError::SubscriptionInvalid { event: event.to_string() }))
                    .await;
            }
        }

        Ok(())
    }
    .instrument(method_span)
    .await
}

// -----------------------------------------------------------------------------
// Storage
// -----------------------------------------------------------------------------

fn eth_get_storage_at(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<String, StratusError> {
    // enter span
    let _middleware_enter = ext.enter_middleware_span();
    let _method_enter = info_span!("rpc::eth_getStorageAt", address = field::Empty, index = field::Empty).entered();

    // parse params
    let (params, address) = next_rpc_param::<Address>(params.sequence())?;
    let (params, index) = next_rpc_param::<SlotIndex>(params)?;
    let (_, block_filter) = next_rpc_param_or_default::<BlockFilter>(params)?;

    Span::with(|s| {
        s.rec_str("address", &address);
        s.rec_str("index", &index);
    });

    // execute
    let point_in_time = ctx.server.storage.translate_to_point_in_time(block_filter)?;
    let slot = ctx.server.storage.read_slot(address, index, point_in_time, ReadKind::RPC)?;

    // It must be padded, even if it is zero.
    Ok(hex_num_zero_padded(slot.value.as_u256()))
}

// -----------------------------------------------------------------------------
// Response helpers
// -----------------------------------------------------------------------------

#[inline(always)]
fn hex_data<T: AsRef<[u8]>>(value: T) -> String {
    const_hex::encode_prefixed(value)
}

#[inline(always)]
fn hex_num(value: impl Into<U256>) -> String {
    format!("{:#x}", value.into())
}

#[inline(always)]
fn hex_num_zero_padded(value: impl Into<U256>) -> String {
    let width = 64 + 2; //the prefix is included in the total width
    format!("{:#0width$x}", value.into(), width = width)
}

fn hex_zero() -> String {
    "0x0".to_owned()
}

fn hex_null() -> String {
    "0x".to_owned()
}

/// Enhances trace using serialized JSON modification
fn enhance_trace_with_decoded_info(trace: &GethTrace) -> JsonValue {
    match trace {
        GethTrace::CallTracer(call_frame) => {
            // Serialize first, then enhance
            let mut json = to_json_value(call_frame);
            enhance_serialized_call_frame(&mut json);
            json
        }
        _ => {
            // For non-CallTracer traces, return as-is without enhancement
            to_json_value(trace)
        }
    }
}

fn enhance_serialized_call_frame(json: &mut JsonValue) {
    if let Some(json_obj) = json.as_object_mut() {
        json_obj
            .get("from")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<Address>().ok())
            .and_then(|addr| CONTRACTS.get(addr.as_slice()).map(|s| s.to_string()))
            .and_then(|s| json_obj.insert("decodedFromContract".to_string(), json!(s)));

        json_obj
            .get("to")
            .and_then(|v| if v.is_null() { None } else { v.as_str() })
            .and_then(|s| s.parse::<Address>().ok())
            .and_then(|addr| CONTRACTS.get(addr.as_slice()).map(|s| s.to_string()))
            .and_then(|s| json_obj.insert("decodedToContract".to_string(), json!(s)));

        json_obj
            .get("input")
            .and_then(|v| v.as_str())
            .and_then(|s| const_hex::decode(s.trim_start_matches("0x")).ok())
            .inspect(|input| {
                if let Some(signature) = codegen::function_sig_opt(input) {
                    json_obj.insert("decodedFunctionSignature".to_string(), json!(signature));
                }

                match decode::decode_input_arguments(input) {
                    Ok(args) => {
                        json_obj.insert("decodedFunctionArguments".to_string(), json!(args));
                    }
                    Err(DecodeInputError::InvalidAbi { message }) => {
                        tracing::warn!(
                            message = %message,
                            "Invalid ABI stored"
                        );
                    }
                    _ => (),
                }
            });
        if let Some(calls) = json_obj.get_mut("calls").and_then(|v| v.as_array_mut()) {
            for call in calls {
                enhance_serialized_call_frame(call);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::Address;
    use alloy_primitives::Bytes;
    use alloy_primitives::U256;
    use alloy_rpc_types_trace::geth::CallFrame;

    use super::*;

    fn create_simple_test_call_structure() -> CallFrame {
        // Create deepest level calls (level 3)
        let deep_call_1 = CallFrame {
            from: "0x562689c910361ae21d12eadafbfca727b3bcbc24".parse::<Address>().unwrap(), // Maps to Compound_Agent_4
            to: Some("0xa9a55a81a4c085ec0c31585aed4cfb09d78dfd53".parse::<Address>().unwrap()), // Maps to BRLCToken
            input: Bytes::from(
                const_hex::decode(
                    "70a08231000000000000000000000000742d35cc6634c0532925a3b8d7c9be8813eeb02e", // balanceOf function
                )
                .unwrap(),
            ),
            output: Some(Bytes::from(
                const_hex::decode("0000000000000000000000000000000000000000000000000de0b6b3a7640000").unwrap(),
            )),
            gas: U256::from(5000),
            gas_used: U256::from(3000),
            value: None,
            typ: "STATICCALL".to_string(),
            error: None,
            revert_reason: None,
            calls: Vec::new(),
            logs: Vec::new(),
        };

        let deep_call_2 = CallFrame {
            from: "0x3181ab023a4d4788754258be5a3b8cf3d8276b98".parse::<Address>().unwrap(), // Maps to Cashier_BRLC_v2
            to: Some("0x6d8da3c039d1d78622f27d4739e1e00b324afaaa".parse::<Address>().unwrap()), // Maps to USJIMToken
            input: Bytes::from(
                const_hex::decode(
                    "dd62ed3e000000000000000000000000742d35cc6634c0532925a3b8d7c9be8813eeb02e000000000000000000000000a0b86a33e6441366ac2ed2e3a8da88e61c66a5e1", // allowance function
                )
                .unwrap(),
            ),
            output: Some(Bytes::from(
                const_hex::decode("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff").unwrap(),
            )),
            gas: U256::from(8000),
            gas_used: U256::from(5000),
            value: None,
            typ: "STATICCALL".to_string(),
            error: None,
            revert_reason: None,
            calls: Vec::new(),
            logs: Vec::new(),
        };

        // Create level 2 nested calls using real contract addresses from CONTRACTS map
        let nested_call_1 = CallFrame {
            from: "0xa9a55a81a4c085ec0c31585aed4cfb09d78dfd53".parse::<Address>().unwrap(), // Maps to BRLCToken
            to: Some("0x6d8da3c039d1d78622f27d4739e1e00b324afaaa".parse::<Address>().unwrap()), // Maps to USJIMToken
            input: Bytes::from(
                const_hex::decode(
                    "a9059cbb000000000000000000000000742d35cc6634c0532925a3b8d7c9be8813eeb02e0000000000000000000000000000000000000000000000000de0b6b3a7640000", // transfer function
                )
                .unwrap(),
            ),
            output: Some(Bytes::from(
                const_hex::decode("0000000000000000000000000000000000000000000000000000000000000001").unwrap(),
            )),
            gas: U256::from(21000),
            gas_used: U256::from(20000),
            value: None,
            typ: "CALL".to_string(),
            error: None,
            revert_reason: None,
            calls: vec![deep_call_1],
            logs: Vec::new(),
        };

        let nested_call_2 = CallFrame {
            from: "0x6d8da3c039d1d78622f27d4739e1e00b324afaaa".parse::<Address>().unwrap(), // Maps to USJIMToken
            to: Some("0x3181ab023a4d4788754258be5a3b8cf3d8276b98".parse::<Address>().unwrap()), // Maps to Cashier_BRLC_v2
            input: Bytes::from(
                const_hex::decode(
                    "095ea7b3000000000000000000000000742d35cc6634c0532925a3b8d7c9be8813eeb02e0000000000000000000000000000000000000000000000000de0b6b3a7640000", // approve function
                )
                .unwrap(),
            ),
            output: Some(Bytes::from(
                const_hex::decode("0000000000000000000000000000000000000000000000000000000000000001").unwrap(),
            )),
            gas: U256::from(30000),
            gas_used: U256::from(25000),
            value: None,
            typ: "CALL".to_string(),
            error: None,
            revert_reason: None,
            calls: vec![deep_call_2],
            logs: Vec::new(),
        };

        // Create main call containing nested calls (level 1)
        CallFrame {
            from: "0x742d35Cc6634C0532925a3b8D7C9be8813eeb02e".parse::<Address>().unwrap(),
            to: Some("0xa9a55a81a4c085ec0c31585aed4cfb09d78dfd53".parse::<Address>().unwrap()), // BRLCToken
            input: Bytes::from(
                const_hex::decode(
                    "23b872dd000000000000000000000000742d35cc6634c0532925a3b8d7c9be8813eeb02e000000000000000000000000a0b86a33e6441366ac2ed2e3a8da88e61c66a5e10000000000000000000000000000000000000000000000000de0b6b3a7640000", // transferFrom function
                )
                .unwrap(),
            ),
            output: Some(Bytes::from(
                const_hex::decode("0000000000000000000000000000000000000000000000000000000000000001").unwrap(),
            )),
            gas: U256::from(100000),
            gas_used: U256::from(85000),
            value: None,
            typ: "CALL".to_string(),
            error: None,
            revert_reason: None,
            calls: vec![nested_call_1, nested_call_2],
            logs: Vec::new(),
        }
    }

    #[test]
    fn test_contract_name_decoding() {
        // Create a 3-level structure using real contract addresses
        let test_call = create_simple_test_call_structure();
        let geth_trace = GethTrace::CallTracer(test_call);

        // Enhance the trace with decoded information
        let result = enhance_trace_with_decoded_info(&geth_trace);
        let result_str = serde_json::to_string_pretty(&result).unwrap();

        assert!(result_str.contains("Cashier_BRLC_v2"));
        assert!(result_str.contains("BRLCToken"));
        assert!(result_str.contains("USJIMToken"));
        assert!(result_str.contains("Compound_Agent_4"));

        // Verify function signature decoding for all the expected functions
        assert!(result_str.contains("transfer(address,uint256)"));
        assert!(result_str.contains("approve(address,uint256)"));
        assert!(result_str.contains("transferFrom(address,address,uint256)"));
        assert!(result_str.contains("balanceOf(address)"));
        assert!(result_str.contains("allowance(address,address)"));
    }
}
