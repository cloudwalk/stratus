//! RPC server for HTTP and WS.

use std::collections::HashMap;
use std::ops::Deref;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use ethereum_types::U256;
use futures::join;
use http::Method;
use itertools::Itertools;
use jsonrpsee::server::middleware::http::ProxyGetRequestLayer;
use jsonrpsee::server::RandomStringIdProvider;
use jsonrpsee::server::RpcModule;
use jsonrpsee::server::RpcServiceBuilder;
use jsonrpsee::server::Server;
use jsonrpsee::types::Params;
use jsonrpsee::Extensions;
use jsonrpsee::IntoSubscriptionCloseResponse;
use jsonrpsee::PendingSubscriptionSink;
use once_cell::sync::Lazy;
use serde_json::json;
use tokio::runtime::Handle;
use tokio::select;
use tokio::sync::Semaphore;
use tokio::sync::SemaphorePermit;
use tower_http::cors::Any;
use tower_http::cors::CorsLayer;
use tracing::field;
use tracing::info_span;
use tracing::Instrument;
use tracing::Span;

use super::rpc_method_wrapper::call_error_metrics_wrapper;
use crate::alias::JsonValue;
use crate::eth::executor::Executor;
use crate::eth::follower::consensus::Consensus;
use crate::eth::follower::importer::ImporterConfig;
use crate::eth::miner::Miner;
use crate::eth::miner::MinerMode;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::CallInput;
use crate::eth::primitives::ChainId;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilterInput;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StratusError;
use crate::eth::primitives::TransactionInput;
use crate::eth::rpc::next_rpc_param;
use crate::eth::rpc::next_rpc_param_or_default;
use crate::eth::rpc::parse_rpc_rlp;
use crate::eth::rpc::rpc_parser::RpcExtensionsExt;
use crate::eth::rpc::RpcClientApp;
use crate::eth::rpc::RpcContext;
use crate::eth::rpc::RpcHttpMiddleware;
use crate::eth::rpc::RpcMiddleware;
use crate::eth::rpc::RpcServerConfig;
use crate::eth::rpc::RpcSubscriptions;
use crate::eth::storage::StoragePointInTime;
use crate::eth::storage::StratusStorage;
use crate::ext::not;
use crate::ext::parse_duration;
use crate::ext::to_json_string;
use crate::ext::to_json_value;
use crate::ext::SerdeResultExt;
use crate::infra::build_info;
use crate::infra::metrics;
use crate::infra::tracing::SpanExt;
use crate::log_and_err;
use crate::GlobalState;
use crate::NodeMode;
// -----------------------------------------------------------------------------
// Server
// -----------------------------------------------------------------------------

/// Starts JSON-RPC server.
#[allow(clippy::too_many_arguments)]
pub async fn serve_rpc(
    // services
    storage: Arc<StratusStorage>,
    executor: Arc<Executor>,
    miner: Arc<Miner>,
    consensus: Option<Arc<dyn Consensus>>,

    // config
    app_config: impl serde::Serialize,
    rpc_config: RpcServerConfig,
    chain_id: ChainId,
) -> anyhow::Result<()> {
    const TASK_NAME: &str = "rpc-server";
    tracing::info!(%rpc_config.rpc_address, %rpc_config.rpc_max_connections, "creating {}", TASK_NAME);

    // configure subscriptions
    let subs = RpcSubscriptions::spawn(
        miner.notifier_pending_txs.subscribe(),
        miner.notifier_blocks.subscribe(),
        miner.notifier_logs.subscribe(),
    );

    // configure context
    let ctx = RpcContext {
        app_config: to_json_value(app_config),
        chain_id,
        client_version: "stratus",
        gas_price: 0,

        // services
        executor,
        storage,
        miner,
        consensus: consensus.into(),
        rpc_server: rpc_config.clone(),

        // subscriptions
        subs: Arc::clone(&subs.connected),
    };

    // configure module
    let mut module = RpcModule::<RpcContext>::new(ctx);
    module = register_methods(module)?;

    // configure middleware
    let cors = CorsLayer::new().allow_methods([Method::POST]).allow_origin(Any).allow_headers(Any);
    let rpc_middleware = RpcServiceBuilder::new().layer_fn(RpcMiddleware::new);
    let http_middleware = tower::ServiceBuilder::new()
        .layer(cors)
        .layer_fn(RpcHttpMiddleware::new)
        .layer(ProxyGetRequestLayer::new("/health", "stratus_health").unwrap())
        .layer(ProxyGetRequestLayer::new("/version", "stratus_version").unwrap())
        .layer(ProxyGetRequestLayer::new("/config", "stratus_config").unwrap())
        .layer(ProxyGetRequestLayer::new("/state", "stratus_state").unwrap());

    // serve module
    let server = Server::builder()
        .set_rpc_middleware(rpc_middleware)
        .set_http_middleware(http_middleware)
        .set_id_provider(RandomStringIdProvider::new(8))
        .max_connections(rpc_config.rpc_max_connections)
        .build(rpc_config.rpc_address)
        .await?;

    let handle_rpc_server = server.start(module);
    let handle_rpc_server_watch = handle_rpc_server.clone();

    // await for cancellation or jsonrpsee to stop (should not happen)
    select! {
        _ = handle_rpc_server_watch.stopped() => {
            GlobalState::shutdown_from(TASK_NAME, "finished unexpectedly");
        },
        _ = GlobalState::wait_shutdown_warn(TASK_NAME) => {
            let _ = handle_rpc_server.stop();
        }
    }

    // await rpc server and subscriptions to finish
    join!(handle_rpc_server.stopped(), subs.handles.stopped());

    Ok(())
}

fn register_methods(mut module: RpcModule<RpcContext>) -> anyhow::Result<RpcModule<RpcContext>> {
    // dev mode methods
    #[cfg(feature = "dev")]
    {
        module.register_blocking_method("evm_setNextBlockTimestamp", evm_set_next_block_timestamp)?;
        module.register_blocking_method("evm_mine", evm_mine)?;
        module.register_blocking_method("hardhat_reset", stratus_reset)?;
        module.register_blocking_method("stratus_reset", stratus_reset)?;
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

    // block
    module.register_blocking_method("eth_blockNumber", eth_block_number)?;
    module.register_blocking_method("eth_getBlockByNumber", eth_get_block_by_number)?;
    module.register_blocking_method("eth_getBlockByHash", eth_get_block_by_hash)?;
    module.register_method("eth_getUncleByBlockHashAndIndex", eth_get_uncle_by_block_hash_and_index)?;

    // transactions
    module.register_blocking_method("eth_getTransactionByHash", eth_get_transaction_by_hash)?;
    module.register_blocking_method("eth_getTransactionReceipt", eth_get_transaction_receipt)?;
    module.register_blocking_method("eth_estimateGas", eth_estimate_gas)?;
    module.register_blocking_method("eth_call", call_error_metrics_wrapper(eth_call))?;
    module.register_blocking_method("eth_sendRawTransaction", call_error_metrics_wrapper(eth_send_raw_transaction))?;

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
    ctx.miner.mine_local_and_commit()?;
    Ok(to_json_value(true))
}

#[cfg(feature = "dev")]
fn evm_set_next_block_timestamp(params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> Result<JsonValue, StratusError> {
    use crate::eth::primitives::UnixTime;
    use crate::log_and_err;

    let (_, timestamp) = next_rpc_param::<UnixTime>(params.sequence())?;
    let latest = ctx.storage.read_block(&BlockFilter::Latest)?;
    match latest {
        Some(block) => UnixTime::set_offset(timestamp, block.header.timestamp)?,
        None => return log_and_err!("reading latest block returned None")?,
    }
    Ok(to_json_value(timestamp))
}

// -----------------------------------------------------------------------------
// Status - Health checks
// -----------------------------------------------------------------------------

async fn stratus_health(_: Params<'_>, context: Arc<RpcContext>, _: Extensions) -> Result<JsonValue, StratusError> {
    if GlobalState::is_shutdown() {
        tracing::warn!("liveness check failed because of shutdown");
        return Err(StratusError::StratusShutdown);
    }

    let should_serve = match GlobalState::get_node_mode() {
        NodeMode::Leader => true,
        NodeMode::Follower => match context.consensus() {
            Some(consensus) => consensus.should_serve().await,
            None => false,
        },
    };

    if not(should_serve) {
        tracing::warn!("readiness check failed because consensus is not ready");
        metrics::set_consensus_is_ready(0_u64);
        return Err(StratusError::StratusNotReady);
    }

    metrics::set_consensus_is_ready(1_u64);
    Ok(json!(true))
}

// -----------------------------------------------------------------------------
// Stratus - Admin
// -----------------------------------------------------------------------------

#[cfg(feature = "dev")]
fn stratus_reset(_: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> Result<JsonValue, StratusError> {
    ctx.storage.reset_to_genesis()?;
    Ok(to_json_value(true))
}

static MODE_CHANGE_SEMAPHORE: Lazy<Semaphore> = Lazy::new(|| Semaphore::new(1));

async fn stratus_change_to_leader(_: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, StratusError> {
    let permit = MODE_CHANGE_SEMAPHORE.try_acquire();
    let _permit: SemaphorePermit = match permit {
        Ok(permit) => permit,
        Err(_) => return Err(StratusError::ModeChangeInProgress),
    };

    const LEADER_MINER_INTERVAL: Duration = Duration::from_secs(1);
    tracing::info!("starting process to change node to leader");

    if GlobalState::get_node_mode() == NodeMode::Leader {
        tracing::info!("node is already in leader mode, no changes made");
        return Ok(json!(false));
    }

    if GlobalState::is_transactions_enabled() {
        tracing::error!("transactions are currently enabled, cannot change node mode");
        return Err(StratusError::RpcTransactionEnabled);
    }

    tracing::info!("shutting down importer");
    let shutdown_importer_result = stratus_shutdown_importer(Params::new(None), &ctx, &ext);
    match shutdown_importer_result {
        Ok(_) => tracing::info!("importer shutdown successfully"),
        Err(StratusError::ImporterAlreadyShutdown) => {
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
    tracing::info!("node mode changed to leader successfully");

    Ok(json!(true))
}

async fn stratus_change_to_follower(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, StratusError> {
    let permit = MODE_CHANGE_SEMAPHORE.try_acquire();
    let _permit: SemaphorePermit = match permit {
        Ok(permit) => permit,
        Err(_) => return Err(StratusError::ModeChangeInProgress),
    };

    tracing::info!("starting process to change node to follower");

    if GlobalState::get_node_mode() == NodeMode::Follower {
        tracing::info!("node is already in follower mode, no changes made");
        return Ok(json!(false));
    }

    if GlobalState::is_transactions_enabled() {
        tracing::error!("transactions are currently enabled, cannot change node mode");
        return Err(StratusError::RpcTransactionEnabled);
    }

    let pending_txs = ctx.storage.pending_transactions();
    if not(pending_txs.is_empty()) {
        tracing::error!(pending_txs = ?pending_txs.len(), "cannot change to follower mode with pending transactions");
        return Err(StratusError::PendingTransactionsExist {
            pending_txs: pending_txs.len(),
        });
    }

    if not(ctx.miner.is_paused()) {
        tracing::error!("miner is currently not paused, cannot change node mode");
        return Err(StratusError::MinerEnabled);
    }

    let change_miner_mode_result = change_miner_mode(MinerMode::External, &ctx).await;
    if let Err(e) = change_miner_mode_result {
        tracing::error!(reason = ?e, "failed to change miner mode");
        return Err(e);
    }
    tracing::info!("miner mode changed to external successfully");

    GlobalState::set_node_mode(NodeMode::Follower);

    tracing::info!("initializing importer");
    let init_importer_result = stratus_init_importer(params, Arc::clone(&ctx), ext).await;
    match init_importer_result {
        Ok(_) => tracing::info!("importer initialized successfully"),
        Err(StratusError::ImporterAlreadyRunning) => {
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

async fn stratus_init_importer(params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> Result<JsonValue, StratusError> {
    let (params, external_rpc) = next_rpc_param::<String>(params.sequence())?;
    let (params, external_rpc_ws) = next_rpc_param::<String>(params)?;
    let (params, raw_external_rpc_timeout) = next_rpc_param::<String>(params)?;
    let (_, raw_sync_interval) = next_rpc_param::<String>(params)?;

    let external_rpc_timeout = parse_duration(&raw_external_rpc_timeout).map_err(|e| {
        tracing::error!(reason = ?e, "failed to parse external_rpc_timeout");
        StratusError::ImporterConfigParseError
    })?;

    let sync_interval = parse_duration(&raw_sync_interval).map_err(|e| {
        tracing::error!(reason = ?e, "failed to parse sync_interval");
        StratusError::ImporterConfigParseError
    })?;

    let importer_config = ImporterConfig {
        external_rpc,
        external_rpc_ws: Some(external_rpc_ws),
        external_rpc_timeout,
        sync_interval,
    };

    importer_config.init_follower_importer(ctx).await
}

fn stratus_shutdown_importer(_: Params<'_>, ctx: &RpcContext, _: &Extensions) -> Result<JsonValue, StratusError> {
    if not(GlobalState::is_follower()) {
        tracing::error!("node is currently not a follower");
        return Err(StratusError::StratusNotFollower);
    }

    if GlobalState::is_importer_shutdown() && ctx.consensus().is_none() {
        tracing::error!("importer is already shut down");
        return Err(StratusError::ImporterAlreadyShutdown);
    }

    ctx.set_consensus(None);

    const TASK_NAME: &str = "rpc-server::importer-shutdown";
    GlobalState::shutdown_importer_from(TASK_NAME, "received importer shutdown request");

    Ok(json!(true))
}

async fn stratus_change_miner_mode(params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> Result<JsonValue, StratusError> {
    let (_, mode_str) = next_rpc_param::<String>(params.sequence())?;
    let mode = MinerMode::from_str(&mode_str).map_err(|e| {
        tracing::error!(reason = ?e, "failed to parse miner mode");
        StratusError::MinerModeParamInvalid
    })?;

    change_miner_mode(mode, &ctx).await
}

/// Tries changing miner mode, returns `Ok(true)` if changed, and `Ok(false)` if no changing was necessary.
///
/// This function also enables the miner after changing it.
async fn change_miner_mode(new_mode: MinerMode, ctx: &RpcContext) -> Result<JsonValue, StratusError> {
    if GlobalState::is_transactions_enabled() {
        tracing::error!("cannot change miner mode while transactions are enabled");
        return Err(StratusError::RpcTransactionEnabled);
    }

    let previous_mode = ctx.miner.mode();

    if previous_mode == new_mode {
        tracing::warn!(?new_mode, current = ?new_mode, "miner mode already set, skipping");
        return Ok(json!(false));
    }

    if not(ctx.miner.is_paused()) && previous_mode.is_interval() {
        return log_and_err!("can't change miner mode from Interval without pausing it first").map_err(Into::into);
    }

    match new_mode {
        MinerMode::External => {
            tracing::info!("changing miner mode to External");

            let pending_txs = ctx.storage.pending_transactions();
            if not(pending_txs.is_empty()) {
                tracing::error!(pending_txs = ?pending_txs.len(), "cannot change miner mode to External with pending transactions");
                return Err(StratusError::PendingTransactionsExist {
                    pending_txs: pending_txs.len(),
                });
            }

            ctx.miner.switch_to_external_mode().await;
        }
        MinerMode::Interval(duration) => {
            tracing::info!(duration = ?duration, "changing miner mode to Interval");

            if ctx.consensus().is_some() {
                tracing::error!("cannot change miner mode to Interval with consensus set");
                return Err(StratusError::ConsensusSet);
            }

            ctx.miner.start_interval_mining(duration).await;
        }
        MinerMode::Automine => {
            tracing::error!("automine mode is not supported");
            return Err(StratusError::MinerModeChangeUnsupported { miner_mode: "automine" });
        }
    }

    Ok(json!(true))
}

fn stratus_enable_unknown_clients(_: Params<'_>, _: &RpcContext, _: &Extensions) -> bool {
    GlobalState::set_unknown_client_enabled(true);
    GlobalState::is_unknown_client_enabled()
}

fn stratus_disable_unknown_clients(_: Params<'_>, _: &RpcContext, _: &Extensions) -> bool {
    GlobalState::set_unknown_client_enabled(false);
    GlobalState::is_unknown_client_enabled()
}

fn stratus_enable_transactions(_: Params<'_>, _: &RpcContext, _: &Extensions) -> bool {
    GlobalState::set_transactions_enabled(true);
    GlobalState::is_transactions_enabled()
}

fn stratus_disable_transactions(_: Params<'_>, _: &RpcContext, _: &Extensions) -> bool {
    GlobalState::set_transactions_enabled(false);
    GlobalState::is_transactions_enabled()
}

fn stratus_enable_miner(_: Params<'_>, ctx: &RpcContext, _: &Extensions) -> bool {
    ctx.miner.unpause();
    true
}

fn stratus_disable_miner(_: Params<'_>, ctx: &RpcContext, _: &Extensions) -> bool {
    ctx.miner.pause();
    false
}

/// Returns the count of executed transactions waiting to enter the next block.
fn stratus_pending_transactions_count(_: Params<'_>, ctx: &RpcContext, _: &Extensions) -> usize {
    ctx.storage.pending_transactions().len()
}

// -----------------------------------------------------------------------------
// Stratus - State
// -----------------------------------------------------------------------------

fn stratus_version(_: Params<'_>, _: &RpcContext, _: &Extensions) -> Result<JsonValue, StratusError> {
    Ok(build_info::as_json())
}

fn stratus_config(_: Params<'_>, ctx: &RpcContext, _: &Extensions) -> Result<JsonValue, StratusError> {
    Ok(ctx.app_config.clone())
}

fn stratus_state(_: Params<'_>, ctx: &RpcContext, _: &Extensions) -> Result<JsonValue, StratusError> {
    Ok(GlobalState::get_global_state_as_json(ctx))
}

async fn stratus_get_subscriptions(_: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, StratusError> {
    reject_unknown_client(ext.rpc_client())?;

    // NOTE: this is a workaround for holding only one lock at a time
    let pending_txs = serde_json::to_value(ctx.subs.new_heads.read().await.values().collect_vec()).expect_infallible();
    let new_heads = serde_json::to_value(ctx.subs.pending_txs.read().await.values().collect_vec()).expect_infallible();
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
    ctx.chain_id.to_string()
}

fn eth_chain_id(_: Params<'_>, ctx: &RpcContext, _: &Extensions) -> String {
    hex_num(ctx.chain_id)
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
    let block_number = ctx.storage.read_mined_block_number()?;
    Span::with(|s| s.rec_str("block_number", &block_number));

    Ok(to_json_value(block_number))
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
    reject_unknown_client(ext.rpc_client())?;
    let (params, filter) = next_rpc_param::<BlockFilter>(params.sequence())?;
    let (_, full_transactions) = next_rpc_param::<bool>(params)?;

    // track
    Span::with(|s| s.rec_str("filter", &filter));
    tracing::info!(%filter, %full_transactions, "reading block");

    // execute
    let block = ctx.storage.read_block(&filter)?;
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
    reject_unknown_client(ext.rpc_client())?;
    let (_, tx_hash) = next_rpc_param::<Hash>(params.sequence())?;

    // track
    Span::with(|s| s.rec_str("tx_hash", &tx_hash));
    tracing::info!(%tx_hash, "reading transaction");

    // execute
    let tx = ctx.storage.read_transaction(&tx_hash)?;
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

fn eth_get_transaction_receipt(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, StratusError> {
    // enter span
    let _middleware_enter = ext.enter_middleware_span();
    let _method_enter = info_span!("rpc::eth_getTransactionReceipt", tx_hash = field::Empty, found = field::Empty).entered();

    // parse params
    reject_unknown_client(ext.rpc_client())?;
    let (_, tx_hash) = next_rpc_param::<Hash>(params.sequence())?;

    // track
    Span::with(|s| s.rec_str("tx_hash", &tx_hash));
    tracing::info!(%tx_hash, "reading transaction receipt");

    // execute
    let tx = ctx.storage.read_transaction(&tx_hash)?;
    Span::with(|s| {
        s.record("found", tx.is_some());
    });

    match tx {
        Some(tx) => {
            tracing::info!(%tx_hash, "transaction receipt found");
            Ok(tx.to_json_rpc_receipt())
        }
        None => {
            tracing::info!(%tx_hash, "transaction receipt not found");
            Ok(JsonValue::Null)
        }
    }
}

fn eth_estimate_gas(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<String, StratusError> {
    // enter span
    let _middleware_enter = ext.enter_middleware_span();
    let _method_enter = info_span!("rpc::eth_estimateGas", tx_from = field::Empty, tx_to = field::Empty).entered();

    // parse params
    reject_unknown_client(ext.rpc_client())?;
    let (_, call) = next_rpc_param::<CallInput>(params.sequence())?;

    // track
    Span::with(|s| {
        s.rec_opt("tx_from", &call.from);
        s.rec_opt("tx_to", &call.to);
    });
    tracing::info!("executing eth_estimateGas");

    // execute
    match ctx.executor.execute_local_call(call, StoragePointInTime::Mined) {
        // result is success
        Ok(result) if result.is_success() => {
            tracing::info!(tx_output = %result.output, "executed eth_estimateGas with success");
            let overestimated_gas = (result.gas.as_u64()) as f64 * 1.1;
            Ok(hex_num(overestimated_gas as u64))
        }

        // result is failure
        Ok(result) => {
            tracing::warn!(tx_output = %result.output, "executed eth_estimateGas with failure");
            Err(StratusError::TransactionReverted { output: result.output })
        }

        // internal error
        Err(e) => {
            if e.is_internal() {
                tracing::error!(reason = ?e, "failed to execute eth_estimateGas");
            }
            Err(e)
        }
    }
}

fn eth_call(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<String, StratusError> {
    // enter span
    let _middleware_enter = ext.enter_middleware_span();
    let _method_enter = info_span!("rpc::eth_call", tx_from = field::Empty, tx_to = field::Empty, filter = field::Empty).entered();

    // parse params
    reject_unknown_client(ext.rpc_client())?;
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
    let point_in_time = ctx.storage.translate_to_point_in_time(&filter)?;
    match ctx.executor.execute_local_call(call, point_in_time) {
        // result is success
        Ok(result) if result.is_success() => {
            tracing::info!(tx_output = %result.output, "executed eth_call with success");
            Ok(hex_data(result.output))
        }

        // result is failure
        Ok(result) => {
            tracing::warn!(tx_output = %result.output, "executed eth_call with failure");
            Err(StratusError::TransactionReverted { output: result.output })
        }

        // internal error
        Err(e) => {
            if e.is_internal() {
                tracing::error!(reason = ?e, "failed to execute eth_call");
            }
            Err(e)
        }
    }
}

fn eth_send_raw_transaction(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<String, StratusError> {
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

    // parse params
    reject_unknown_client(ext.rpc_client())?;
    let (_, tx_data) = next_rpc_param::<Bytes>(params.sequence())?;
    let tx = parse_rpc_rlp::<TransactionInput>(&tx_data)?;
    let tx_hash = tx.hash;

    // track
    Span::with(|s| {
        s.rec_str("tx_hash", &tx_hash);
        s.rec_str("tx_from", &tx.signer);
        s.rec_opt("tx_to", &tx.to);
        s.rec_str("tx_nonce", &tx.nonce);
    });

    // check feature
    if not(GlobalState::is_transactions_enabled()) {
        tracing::warn!(%tx_hash, "failed to execute eth_sendRawTransaction because transactions are disabled");
        return Err(StratusError::RpcTransactionDisabled);
    }

    // execute locally or forward to leader
    match GlobalState::get_node_mode() {
        NodeMode::Leader => match ctx.executor.execute_local_transaction(tx) {
            Ok(_) => Ok(hex_data(tx_hash)),
            Err(e) => {
                if e.is_internal() {
                    tracing::error!(reason = ?e, "failed to execute eth_sendRawTransaction");
                }
                Err(e)
            }
        },
        NodeMode::Follower => match ctx.consensus() {
            Some(consensus) => match Handle::current().block_on(consensus.forward_to_leader(tx_hash, tx_data, ext.rpc_client())) {
                Ok(hash) => Ok(hex_data(hash)),
                Err(e) => Err(e),
            },
            None => {
                tracing::error!("unable to forward transaction because consensus is temporarily unavailable for follower node");
                Err(StratusError::ConsensusUnavailable)
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
    reject_unknown_client(ext.rpc_client())?;
    let (_, filter_input) = next_rpc_param_or_default::<LogFilterInput>(params.sequence())?;
    let mut filter = filter_input.parse(&ctx.storage)?;

    // for this operation, the filter always need the end block specified to calculate the difference
    if filter.to_block.is_none() {
        filter.to_block = Some(ctx.storage.read_mined_block_number()?);
    }
    let blocks_in_range = filter.from_block.count_to(&filter.to_block.unwrap());

    // track
    Span::with(|s| {
        s.rec_str("filter", &to_json_string(&filter));
        s.rec_str("filter_from", &filter.from_block);
        s.rec_str("filter_to", &filter.to_block.unwrap());
        s.rec_str("filter_range", &blocks_in_range);
    });
    tracing::info!(?filter, "reading logs");

    // check range
    if blocks_in_range > MAX_BLOCK_RANGE {
        return Err(StratusError::RpcBlockRangeInvalid {
            actual: blocks_in_range,
            max: MAX_BLOCK_RANGE,
        });
    }

    // execute
    let logs = ctx.storage.read_logs(&filter)?;
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
    reject_unknown_client(ext.rpc_client())?;
    let (params, address) = next_rpc_param::<Address>(params.sequence())?;
    let (_, filter) = next_rpc_param_or_default::<BlockFilter>(params)?;

    // track
    Span::with(|s| {
        s.rec_str("address", &address);
        s.rec_str("address", &filter);
    });
    tracing::info!(%address, %filter, "reading account nonce");

    let point_in_time = ctx.storage.translate_to_point_in_time(&filter)?;
    let account = ctx.storage.read_account(&address, &point_in_time)?;
    Ok(hex_num(account.nonce))
}

fn eth_get_balance(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<String, StratusError> {
    // enter span
    let _middleware_enter = ext.enter_middleware_span();
    let _method_enter = info_span!("rpc::eth_getBalance", address = field::Empty, filter = field::Empty).entered();

    // parse params
    reject_unknown_client(ext.rpc_client())?;
    let (params, address) = next_rpc_param::<Address>(params.sequence())?;
    let (_, filter) = next_rpc_param_or_default::<BlockFilter>(params)?;

    // track
    Span::with(|s| {
        s.rec_str("address", &address);
        s.rec_str("filter", &filter);
    });
    tracing::info!(%address, %filter, "reading account native balance");

    // execute
    let point_in_time = ctx.storage.translate_to_point_in_time(&filter)?;
    let account = ctx.storage.read_account(&address, &point_in_time)?;
    Ok(hex_num(account.balance))
}

fn eth_get_code(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<String, StratusError> {
    // enter span
    let _middleware_enter = ext.enter_middleware_span();
    let _method_enter = info_span!("rpc::eth_getCode", address = field::Empty, filter = field::Empty).entered();

    // parse params
    reject_unknown_client(ext.rpc_client())?;
    let (params, address) = next_rpc_param::<Address>(params.sequence())?;
    let (_, filter) = next_rpc_param_or_default::<BlockFilter>(params)?;

    // track
    Span::with(|s| {
        s.rec_str("address", &address);
        s.rec_str("filter", &filter);
    });

    // execute
    let point_in_time = ctx.storage.translate_to_point_in_time(&filter)?;
    let account = ctx.storage.read_account(&address, &point_in_time)?;

    Ok(account.bytecode.map(hex_data).unwrap_or_else(hex_null))
}

// -----------------------------------------------------------------------------
// Subscriptions
// -----------------------------------------------------------------------------

async fn eth_subscribe(params: Params<'_>, pending: PendingSubscriptionSink, ctx: Arc<RpcContext>, ext: Extensions) -> impl IntoSubscriptionCloseResponse {
    // enter span
    let _middleware_enter = ext.enter_middleware_span();
    let method_span = info_span!("rpc::eth_subscribe", subscription = field::Empty);
    let method_enter = method_span.enter();

    // parse params
    reject_unknown_client(ext.rpc_client())?;
    let client = ext.rpc_client();
    let (params, event) = match next_rpc_param::<String>(params.sequence()) {
        Ok((params, event)) => (params, event),
        Err(e) => {
            drop(method_enter);
            pending.reject(e).instrument(method_span).await;
            return Ok(());
        }
    };

    // check subscription limits
    if let Err(e) = ctx.subs.check_client_subscriptions(ctx.rpc_server.rpc_max_subscriptions, &client).await {
        drop(method_enter);
        pending.reject(e).instrument(method_span).await;
        return Ok(());
    }

    // track
    Span::with(|s| s.rec_str("subscription", &event));
    tracing::info!(%event, "subscribing to rpc event");

    // execute
    match event.deref() {
        "newPendingTransactions" => {
            drop(method_enter);
            ctx.subs
                .add_new_pending_txs_subscription(client, pending.accept().await?)
                .instrument(method_span)
                .await;
        }

        "newHeads" => {
            drop(method_enter);
            ctx.subs
                .add_new_heads_subscription(client, pending.accept().await?)
                .instrument(method_span)
                .await;
        }

        "logs" => {
            let (_, filter) = next_rpc_param_or_default::<LogFilterInput>(params)?;
            let filter = filter.parse(&ctx.storage)?;
            drop(method_enter);
            ctx.subs
                .add_logs_subscription(client, filter, pending.accept().await?)
                .instrument(method_span)
                .await;
        }

        // unsupported
        event => {
            drop(method_enter);
            pending
                .reject(StratusError::RpcSubscriptionInvalid { event: event.to_string() })
                .instrument(method_span)
                .await;
        }
    };
    Ok(())
}

// -----------------------------------------------------------------------------
// Storage
// -----------------------------------------------------------------------------

fn eth_get_storage_at(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<String, StratusError> {
    // enter span
    let _middleware_enter = ext.enter_middleware_span();
    let _method_enter = info_span!("rpc::eth_getStorageAt", address = field::Empty, index = field::Empty).entered();

    // parse params
    reject_unknown_client(ext.rpc_client())?;
    let (params, address) = next_rpc_param::<Address>(params.sequence())?;
    let (params, index) = next_rpc_param::<SlotIndex>(params)?;
    let (_, block_filter) = next_rpc_param_or_default::<BlockFilter>(params)?;

    Span::with(|s| {
        s.rec_str("address", &address);
        s.rec_str("index", &index);
    });

    // execute
    let point_in_time = ctx.storage.translate_to_point_in_time(&block_filter)?;
    let slot = ctx.storage.read_slot(&address, &index, &point_in_time)?;

    // It must be padded, even if it is zero.
    Ok(hex_num_zero_padded(slot.value.as_u256()))
}

// -----------------------------------------------------------------------------
// Request helpers
// -----------------------------------------------------------------------------

/// Returns an error JSON-RPC response if the client is not allowed to perform the current operation.
fn reject_unknown_client(client: RpcClientApp) -> Result<(), StratusError> {
    if client.is_unknown() && not(GlobalState::is_unknown_client_enabled()) {
        return Err(StratusError::RpcClientMissing);
    }
    Ok(())
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
