//! RPC server for HTTP and WS.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::ops::Deref;
use std::sync::Arc;

use ethereum_types::U256;
use futures::join;
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
use serde_json::json;
use tokio::runtime::Handle;
use tokio::select;
use tracing::field;
use tracing::info_span;
use tracing::Instrument;
use tracing::Span;

use crate::eth::executor::Executor;
use crate::eth::miner::Miner;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockFilter;
#[cfg(feature = "dev")]
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::CallInput;
use crate::eth::primitives::ChainId;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilterInput;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionInput;
use crate::eth::rpc::next_rpc_param;
use crate::eth::rpc::next_rpc_param_or_default;
use crate::eth::rpc::parse_rpc_rlp;
use crate::eth::rpc::rpc_parser::RpcExtensionsExt;
use crate::eth::rpc::RpcClientApp;
use crate::eth::rpc::RpcContext;
use crate::eth::rpc::RpcError;
use crate::eth::rpc::RpcHttpMiddleware;
use crate::eth::rpc::RpcMiddleware;
use crate::eth::rpc::RpcSubscriptions;
use crate::eth::storage::StratusStorage;
use crate::eth::Consensus;
use crate::ext::not;
use crate::ext::to_json_string;
use crate::ext::to_json_value;
use crate::ext::JsonValue;
use crate::infra::build_info;
use crate::infra::metrics;
use crate::infra::tracing::SpanExt;
use crate::GlobalState;

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
    consensus: Arc<Consensus>,
    // config
    address: SocketAddr,
    chain_id: ChainId,
    max_connections: u32,
    #[cfg(feature = "request-replication-test-sender")] replication_sender: tokio::sync::mpsc::UnboundedSender<serde_json::Value>,
) -> anyhow::Result<()> {
    const TASK_NAME: &str = "rpc-server";
    tracing::info!(%address, %max_connections, "creating {}", TASK_NAME);

    // configure subscriptions
    let subs = RpcSubscriptions::spawn(
        miner.notifier_pending_txs.subscribe(),
        miner.notifier_blocks.subscribe(),
        miner.notifier_logs.subscribe(),
    );

    // configure context
    let ctx = RpcContext {
        chain_id,
        client_version: "stratus",
        gas_price: 0,

        // services
        executor,
        storage,
        miner,
        consensus,

        // subscriptions
        subs: Arc::clone(&subs.connected),
    };

    // configure module
    let mut module = RpcModule::<RpcContext>::new(ctx);
    module = register_methods(module)?;

    // configure middleware
    let rpc_middleware = RpcServiceBuilder::new().layer_fn(move |service| {
        RpcMiddleware::new(
            service,
            #[cfg(feature = "request-replication-test-sender")]
            replication_sender.clone(),
        )
    });
    let http_middleware = tower::ServiceBuilder::new()
        .layer_fn(RpcHttpMiddleware::new)
        .layer(ProxyGetRequestLayer::new("/startup", "stratus_startup").unwrap())
        .layer(ProxyGetRequestLayer::new("/readiness", "stratus_readiness").unwrap())
        .layer(ProxyGetRequestLayer::new("/liveness", "stratus_liveness").unwrap())
        .layer(ProxyGetRequestLayer::new("/health", "stratus_health").unwrap())
        .layer(ProxyGetRequestLayer::new("/version", "stratus_version").unwrap());

    // serve module
    let server = Server::builder()
        .set_rpc_middleware(rpc_middleware)
        .set_http_middleware(http_middleware)
        .set_id_provider(RandomStringIdProvider::new(8))
        .max_connections(max_connections)
        .build(address)
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
    // debug
    #[cfg(feature = "dev")]
    {
        module.register_blocking_method("evm_setNextBlockTimestamp", evm_set_next_block_timestamp)?;
        module.register_blocking_method("evm_mine", evm_mine)?;
        module.register_blocking_method("debug_setHead", debug_set_head)?;
    }

    // stratus status
    module.register_method("stratus_startup", stratus_startup)?;
    module.register_async_method("stratus_readiness", stratus_readiness)?;
    module.register_method("stratus_liveness", stratus_liveness)?;
    module.register_async_method("stratus_health", stratus_health)?;
    module.register_method("stratus_version", stratus_version)?;

    // stratus state
    module.register_method("stratus_enableTransactions", stratus_enable_transactions)?;
    module.register_method("stratus_disableTransactions", stratus_disable_transactions)?;
    module.register_method("stratus_enableMiner", stratus_enable_miner)?;
    module.register_method("stratus_disableMiner", stratus_disable_miner)?;
    module.register_method("stratus_enableUnknownClients", stratus_enable_unknown_clients)?;
    module.register_method("stratus_disableUnknownClients", stratus_disable_unknown_clients)?;

    module.register_blocking_method("stratus_getSlots", stratus_get_slots)?;
    module.register_async_method("stratus_getSubscriptions", stratus_get_subscriptions)?;

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
    module.register_blocking_method("eth_call", eth_call)?;
    module.register_blocking_method("eth_sendRawTransaction", eth_send_raw_transaction)?;

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
fn debug_set_head(params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> Result<JsonValue, RpcError> {
    let (_, number) = next_rpc_param::<BlockNumber>(params.sequence())?;
    ctx.storage.reset(number)?;
    Ok(to_json_value(number))
}

#[cfg(feature = "dev")]
fn evm_mine(_params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> Result<JsonValue, RpcError> {
    ctx.miner.mine_local_and_commit()?;
    Ok(to_json_value(true))
}

#[cfg(feature = "dev")]
fn evm_set_next_block_timestamp(params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> Result<JsonValue, RpcError> {
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
// Status
// -----------------------------------------------------------------------------

fn stratus_startup(_: Params<'_>, _: &RpcContext, _: &Extensions) -> Result<JsonValue, RpcError> {
    Ok(json!(true))
}

async fn stratus_readiness(_: Params<'_>, context: Arc<RpcContext>, _: Extensions) -> Result<JsonValue, RpcError> {
    let should_serve = context.consensus.should_serve().await;
    if not(should_serve) {
        tracing::warn!("readiness check failed because consensus is not ready");
        metrics::set_consensus_is_ready(0_u64);
        return Err(RpcError::StratusNotReady);
    }

    metrics::set_consensus_is_ready(1_u64);
    Ok(json!(true))
}

fn stratus_liveness(_: Params<'_>, _: &RpcContext, _: &Extensions) -> Result<JsonValue, RpcError> {
    if GlobalState::is_shutdown() {
        tracing::warn!("liveness check failed because of shutdown");
        return Err(RpcError::StratusShutdown);
    }

    Ok(json!(true))
}

/// If stratus is ready and able to receive traffic.
///
/// This is an `AND` of `readiness` with `liveness`.
async fn stratus_health(params: Params<'_>, context: Arc<RpcContext>, extensions: Extensions) -> Result<JsonValue, RpcError> {
    stratus_liveness(params.clone(), &context, &extensions)?;
    stratus_readiness(params, context, extensions).await?;
    Ok(json!(true))
}

fn stratus_version(_: Params<'_>, ctx: &RpcContext, _: &Extensions) -> Result<JsonValue, RpcError> {
    Ok(build_info::as_json(ctx))
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

fn stratus_enable_miner(_: Params<'_>, _: &RpcContext, _: &Extensions) -> bool {
    GlobalState::set_miner_enabled(true);
    GlobalState::is_miner_enabled()
}

fn stratus_disable_miner(_: Params<'_>, _: &RpcContext, _: &Extensions) -> bool {
    GlobalState::set_miner_enabled(false);
    GlobalState::is_miner_enabled()
}

fn stratus_get_slots(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<Vec<Slot>, RpcError> {
    // enter span
    let _middleware_enter = ext.enter_middleware_span();
    let _method_enter = info_span!("rpc::stratus_getSlots", address = field::Empty, indexes = field::Empty).entered();

    // parse params
    reject_unknown_client(ext.rpc_client())?;
    let (params, address) = next_rpc_param::<Address>(params.sequence())?;
    let (params, indexes) = next_rpc_param_or_default::<Vec<SlotIndex>>(params)?;
    let (_, block_filter) = next_rpc_param_or_default::<BlockFilter>(params)?;

    // track
    Span::with(|s| {
        s.rec_str("address", &address);
        s.rec_str("index", &format!("{:?}", indexes));
    });

    // execute
    // no indexes specified, read all slots
    let point_in_time = ctx.storage.translate_to_point_in_time(&block_filter)?;
    match indexes.len() {
        // no indexes specified, real all slots
        0 => {
            tracing::info!(%address, ?indexes, indexes_len = %indexes.len(), %point_in_time, "reading all account slots");
            let all_slots = ctx.storage.read_all_slots(&address, &point_in_time)?;
            Ok(all_slots)
        }
        // indexes specified, read only the ones specified
        _ => {
            tracing::info!(%address, ?indexes, indexes_len = %indexes.len(), %point_in_time, "reading selected account slots");
            let mut selected_slots = Vec::with_capacity(indexes.len());
            for index in indexes {
                let slot = ctx.storage.read_slot(&address, &index, &point_in_time)?;
                selected_slots.push(slot);
            }
            Ok(selected_slots)
        }
    }
}

async fn stratus_get_subscriptions(_: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, RpcError> {
    reject_unknown_client(ext.rpc_client())?;

    let (pending_txs, new_heads, logs) = join!(ctx.subs.new_heads.read(), ctx.subs.pending_txs.read(), ctx.subs.logs.read());
    let response = json!({
        "newPendingTransactions":
            pending_txs.values().map(|s|
                json!({
                    "created_at": s.created_at,
                    "client": s.client,
                    "id": s.sink.subscription_id(),
                    "active": not(s.sink.is_closed())
                })
            ).collect_vec()
        ,
        "newHeads":
            new_heads.values().map(|s|
                json!({
                    "created_at": s.created_at,
                    "client": s.client,
                    "id": s.sink.subscription_id(),
                    "active": not(s.sink.is_closed())
                })
            ).collect_vec()
        ,
        "logs":
            logs.values().flat_map(HashMap::values).map(|s|
                json!({
                    "created_at": s.created_at,
                    "client": s.client,
                    "id": s.sink.subscription_id(),
                    "active": not(s.sink.is_closed()),
                    "filter": {
                        "parsed": s.filter,
                        "original": s.filter.original_input
                    }
                })
            ).collect_vec()
    });
    Ok(response)
}

// -----------------------------------------------------------------------------
// Blockchain
// -----------------------------------------------------------------------------

async fn net_listening(params: Params<'_>, arc: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, RpcError> {
    stratus_readiness(params, arc, ext).await
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

fn eth_block_number(_params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, RpcError> {
    // enter span
    let _middleware_enter = ext.enter_middleware_span();
    let _method_enter = info_span!("rpc::eth_blockNumber", block_number = field::Empty).entered();

    // execute
    let block_number = ctx.storage.read_mined_block_number()?;
    Span::with(|s| s.rec_str("block_number", &block_number));

    Ok(to_json_value(block_number))
}

fn eth_get_block_by_hash(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, RpcError> {
    eth_get_block_by_selector::<'h'>(params, ctx, ext)
}

fn eth_get_block_by_number(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, RpcError> {
    eth_get_block_by_selector::<'n'>(params, ctx, ext)
}

#[inline(always)]
fn eth_get_block_by_selector<const KIND: char>(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, RpcError> {
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

fn eth_get_uncle_by_block_hash_and_index(_: Params<'_>, _: &RpcContext, _: &Extensions) -> Result<JsonValue, RpcError> {
    Ok(JsonValue::Null)
}

// -----------------------------------------------------------------------------
// Transaction
// -----------------------------------------------------------------------------

fn eth_get_transaction_by_hash(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, RpcError> {
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

fn eth_get_transaction_receipt(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, RpcError> {
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

fn eth_estimate_gas(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<String, RpcError> {
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
            Err(RpcError::TransactionReverted { output: result.output })
        }

        // internal error
        Err(e) => {
            tracing::error!(reason = ?e, "failed to execute eth_estimateGas because of unexpected error");
            Err(error_with_source(e, "failed to execute eth_estimateGas"))
        }
    }
}

fn eth_call(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<String, RpcError> {
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
            Err(RpcError::TransactionReverted { output: result.output })
        }

        // internal error
        Err(e) => {
            tracing::error!(reason = ?e, "failed to execute eth_call because of unexpected error");
            Err(error_with_source(e, "failed to execute eth_call"))
        }
    }
}

fn eth_send_raw_transaction(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<String, RpcError> {
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
    let (_, data) = next_rpc_param::<Bytes>(params.sequence())?;
    let tx = parse_rpc_rlp::<TransactionInput>(&data)?;

    // track
    Span::with(|s| {
        s.rec_str("tx_hash", &tx.hash);
        s.rec_str("tx_from", &tx.signer);
        s.rec_opt("tx_to", &tx.to);
        s.rec_str("tx_nonce", &tx.nonce);
    });
    let tx_hash = tx.hash;

    // forward transaction to the leader
    if ctx.consensus.should_forward() {
        tracing::info!(%tx_hash, "forwarding eth_sendRawTransaction to leader");
        return match Handle::current().block_on(ctx.consensus.forward(data)) {
            Ok((hash, url)) => {
                tracing::info!(%tx_hash, %url, "forwarded eth_sendRawTransaction to leader");
                Ok(hex_data(hash))
            }
            Err(e) => {
                tracing::error!(reason = ?e, %tx_hash, "failed to forward eth_sendRawTransaction to leader");
                Err(RpcError::TransactionForwardFailed)
            }
        };
    }

    // check feature
    if not(GlobalState::is_transactions_enabled()) {
        tracing::warn!(%tx_hash, "failed to execute eth_sendRawTransaction because transactions are disabled");
        return Err(RpcError::TransactionDisabled);
    }

    // execute locally if leader
    tracing::info!(%tx_hash, "executing eth_sendRawTransaction locally");
    match ctx.executor.execute_local_transaction(tx) {
        Ok(tx) => {
            if tx.is_success() {
                tracing::info!(tx_hash = %tx.hash(), tx_output = %tx.execution().output, "executed eth_sendRawTransaction with success");
            } else {
                tracing::warn!(tx_output = %tx.hash(), tx_output = %tx.execution().output, "executed eth_sendRawTransaction with failure");
            }
            Ok(hex_data(tx_hash))
        }
        Err(e) => {
            tracing::error!(reason = ?e, "failed to execute eth_sendRawTransaction because of unexpected error");
            Err(error_with_source(e, "failed to execute eth_sendRawTransaction"))
        }
    }
}

// -----------------------------------------------------------------------------
// Logs
// -----------------------------------------------------------------------------

fn eth_get_logs(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<JsonValue, RpcError> {
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
        return Err(RpcError::BlockRangeInvalid {
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

fn eth_accounts(_: Params<'_>, _ctx: &RpcContext, _: &Extensions) -> Result<JsonValue, RpcError> {
    Ok(json!([]))
}

fn eth_get_transaction_count(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<String, RpcError> {
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

fn eth_get_balance(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<String, RpcError> {
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

fn eth_get_code(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<String, RpcError> {
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

    // track
    Span::with(|s| s.rec_str("subscription", &event));
    tracing::info!(%event, "subscribing to rpc event");

    // execute
    match event.deref() {
        "newPendingTransactions" => {
            drop(method_enter);
            ctx.subs.add_new_pending_txs(client, pending.accept().await?).instrument(method_span).await;
        }

        "newHeads" => {
            drop(method_enter);
            ctx.subs.add_new_heads(client, pending.accept().await?).instrument(method_span).await;
        }

        "logs" => {
            let (_, filter) = next_rpc_param_or_default::<LogFilterInput>(params)?;
            let filter = filter.parse(&ctx.storage)?;
            drop(method_enter);
            ctx.subs.add_logs(client, filter, pending.accept().await?).instrument(method_span).await;
        }

        // unsupported
        event => {
            drop(method_enter);
            pending
                .reject(RpcError::SubscriptionInvalid { event: event.to_string() })
                .instrument(method_span)
                .await;
        }
    };
    Ok(())
}

// -----------------------------------------------------------------------------
// Storage
// -----------------------------------------------------------------------------

fn eth_get_storage_at(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> Result<String, RpcError> {
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
fn reject_unknown_client(client: RpcClientApp) -> Result<(), RpcError> {
    if client.is_unknown() && not(GlobalState::is_unknown_client_enabled()) {
        return Err(RpcError::ClientMissing);
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

/// TODO: remove
#[inline(always)]
fn error_with_source(e: anyhow::Error, context: &str) -> RpcError {
    let error_source = format!("{:?}", e.source());
    e.context(format!("{} {}", context, error_source)).into()
}

fn hex_zero() -> String {
    "0x0".to_owned()
}

fn hex_null() -> String {
    "0x".to_owned()
}
