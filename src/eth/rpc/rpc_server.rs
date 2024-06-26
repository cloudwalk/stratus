//! RPC server for HTTP and WS.

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
use serde_json::Value as JsonValue;
use tokio::runtime::Handle;
use tokio::select;
use tracing::Span;

use crate::eth::primitives::Address;
#[cfg(feature = "dev")]
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::CallInput;
use crate::eth::primitives::ChainId;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilterInput;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionInput;
use crate::eth::rpc::next_rpc_param;
use crate::eth::rpc::next_rpc_param_or_default;
use crate::eth::rpc::parse_rpc_rlp;
use crate::eth::rpc::rpc_internal_error;
use crate::eth::rpc::rpc_invalid_params_error;
use crate::eth::rpc::rpc_parser::RpcExtensionsExt;
use crate::eth::rpc::RpcContext;
use crate::eth::rpc::RpcError;
use crate::eth::rpc::RpcHttpMiddleware;
use crate::eth::rpc::RpcMiddleware;
use crate::eth::rpc::RpcSubscriptions;
use crate::eth::storage::StratusStorage;
use crate::eth::BlockMiner;
use crate::eth::Consensus;
use crate::eth::Executor;
use crate::ext::not;
use crate::ext::to_json_value;
use crate::infra::build_info;
#[cfg(feature = "metrics")]
use crate::infra::metrics;
use crate::infra::tracing::warn_task_cancellation;
use crate::infra::tracing::SpanExt;
use crate::GlobalState;

// -----------------------------------------------------------------------------
// Server
// -----------------------------------------------------------------------------

/// Starts JSON-RPC server.
pub async fn serve_rpc(
    // services
    storage: Arc<StratusStorage>,
    executor: Arc<Executor>,
    miner: Arc<BlockMiner>,
    consensus: Arc<Consensus>,
    // config
    address: SocketAddr,
    chain_id: ChainId,
    max_connections: u32,
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
    let rpc_middleware = RpcServiceBuilder::new().layer_fn(RpcMiddleware::new);
    let http_middleware = tower::ServiceBuilder::new()
        .layer_fn(RpcHttpMiddleware::new)
        .layer(ProxyGetRequestLayer::new("/startup", "stratus_startup").unwrap())
        .layer(ProxyGetRequestLayer::new("/readiness", "stratus_readiness").unwrap())
        .layer(ProxyGetRequestLayer::new("/liveness", "stratus_liveness").unwrap())
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
        _ = GlobalState::until_shutdown() => {
            warn_task_cancellation(TASK_NAME);
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
        module.register_blocking_method("debug_readAllSlotsFromAccount", debug_read_all_slots)?;
    }
    module.register_async_method("debug_readSubscriptions", debug_read_subscriptions)?;

    // stratus health check
    module.register_method("stratus_startup", stratus_startup)?;
    module.register_async_method("stratus_readiness", stratus_readiness)?;
    module.register_method("stratus_liveness", stratus_liveness)?;
    module.register_method("stratus_version", stratus_version)?;

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
fn debug_set_head(params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> anyhow::Result<JsonValue, RpcError> {
    let (_, number) = next_rpc_param::<BlockNumber>(params.sequence())?;
    ctx.storage.reset(number)?;
    Ok(to_json_value(number))
}

#[cfg(feature = "dev")]
fn evm_mine(_params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> anyhow::Result<JsonValue, RpcError> {
    ctx.miner.mine_local_and_commit()?;
    Ok(to_json_value(true))
}

#[cfg(feature = "dev")]
fn evm_set_next_block_timestamp(params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> anyhow::Result<JsonValue, RpcError> {
    use crate::eth::primitives::UnixTime;
    use crate::log_and_err;

    let (_, timestamp) = next_rpc_param::<UnixTime>(params.sequence())?;
    let latest = ctx.storage.read_block(&BlockSelection::Latest)?;
    match latest {
        Some(block) => UnixTime::set_offset(timestamp, block.header.timestamp)?,
        None => return log_and_err!("reading latest block returned None")?,
    }
    Ok(to_json_value(timestamp))
}

#[cfg(feature = "dev")]
fn debug_read_all_slots(params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> anyhow::Result<JsonValue, RpcError> {
    let (_, address) = next_rpc_param::<Address>(params.sequence())?;
    let slots = ctx.storage.read_all_slots(&address)?;
    Ok(to_json_value(slots))
}

async fn debug_read_subscriptions(_: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> JsonValue {
    let (pending_txs, new_heads, logs) = join!(ctx.subs.pending_txs.read(), ctx.subs.new_heads.read(), ctx.subs.logs.read());
    json!({
        "newPendingTransactions":
            pending_txs.values().map(|s|
                json!({
                    "client": s.client,
                    "id": s.sink.subscription_id(),
                    "active": not(s.sink.is_closed())
                })
            ).collect_vec()
        ,
        "newHeads":
            new_heads.values().map(|s|
                json!({
                    "client": s.client,
                    "id": s.sink.subscription_id(),
                    "active": not(s.sink.is_closed())
                })
            ).collect_vec()
        ,
        "logs":
            logs.values().map(|s|
                json!({
                    "client": s.client,
                    "id": s.sink.subscription_id(),
                    "active": not(s.sink.is_closed()),
                    "filter": {
                        "parsed": s.filter,
                        "original": s.filter.original_input
                    }
                })
            ).collect_vec()
    })
}

// -----------------------------------------------------------------------------
// Status
// -----------------------------------------------------------------------------

fn stratus_startup(_: Params<'_>, _: &RpcContext, _: &Extensions) -> anyhow::Result<JsonValue, RpcError> {
    Ok(json!(true))
}

async fn stratus_readiness(_: Params<'_>, context: Arc<RpcContext>, _: Extensions) -> anyhow::Result<JsonValue, RpcError> {
    let should_serve = context.consensus.should_serve().await;
    tracing::info!("stratus_readiness: {}", should_serve);

    if should_serve {
        #[cfg(feature = "metrics")]
        metrics::set_consensus_is_ready(1_u64);
        Ok(json!(true))
    } else {
        #[cfg(feature = "metrics")]
        metrics::set_consensus_is_ready(0_u64);
        Err(rpc_internal_error("Service Not Ready".to_string()).into())
    }
}

fn stratus_liveness(_: Params<'_>, _: &RpcContext, _: &Extensions) -> anyhow::Result<JsonValue, RpcError> {
    if !GlobalState::is_shutdown() {
        Ok(json!(true))
    } else {
        Err(rpc_internal_error("Service Unhealthy".to_string()).into())
    }
}

fn stratus_version(_: Params<'_>, _: &RpcContext, _: &Extensions) -> anyhow::Result<JsonValue, RpcError> {
    Ok(build_info::as_json())
}

// -----------------------------------------------------------------------------
// Blockchain
// -----------------------------------------------------------------------------

async fn net_listening(params: Params<'_>, arc: Arc<RpcContext>, ext: Extensions) -> anyhow::Result<JsonValue, RpcError> {
    stratus_readiness(params, arc, ext).await
}

#[tracing::instrument(name = "rpc::net_version", skip_all)]
fn net_version(_: Params<'_>, ctx: &RpcContext, _: &Extensions) -> String {
    ctx.chain_id.to_string()
}

#[tracing::instrument(name = "rpc::eth_chainId", skip_all)]
fn eth_chain_id(_: Params<'_>, ctx: &RpcContext, _: &Extensions) -> String {
    hex_num(ctx.chain_id)
}

#[tracing::instrument(name = "rpc::web3_clientVersion", skip_all)]
fn web3_client_version(_: Params<'_>, ctx: &RpcContext, _: &Extensions) -> String {
    ctx.client_version.to_owned()
}

// -----------------------------------------------------------------------------
// Gas
// -----------------------------------------------------------------------------

#[tracing::instrument(name = "rpc::eth_gasPrice", skip_all)]
fn eth_gas_price(_: Params<'_>, _: &RpcContext, _: &Extensions) -> String {
    hex_zero()
}

// -----------------------------------------------------------------------------
// Block
// -----------------------------------------------------------------------------

#[tracing::instrument(name = "rpc::eth_blockNumber", skip_all)]
fn eth_block_number(_params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> anyhow::Result<JsonValue, RpcError> {
    let number = ctx.storage.read_mined_block_number()?;
    Ok(to_json_value(number))
}

#[tracing::instrument(name = "rpc::eth_getBlockByHash", skip_all, fields(filter, found, number))]
fn eth_get_block_by_hash(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> anyhow::Result<JsonValue, RpcError> {
    eth_get_block_by_selector(params, ctx, ext)
}

#[tracing::instrument(name = "rpc::eth_getBlockByNumber", skip_all, fields(filter, found, number))]
fn eth_get_block_by_number(params: Params<'_>, ctx: Arc<RpcContext>, ext: Extensions) -> anyhow::Result<JsonValue, RpcError> {
    eth_get_block_by_selector(params, ctx, ext)
}

#[inline(always)]
fn eth_get_block_by_selector(params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> anyhow::Result<JsonValue, RpcError> {
    let (params, block_selection) = next_rpc_param::<BlockSelection>(params.sequence())?;
    let (_, full_transactions) = next_rpc_param::<bool>(params)?;

    Span::with(|s| match block_selection {
        BlockSelection::Hash(hash) => s.rec_str("filter", &hash),
        BlockSelection::Latest => s.rec_str("filter", &"latest"),
        BlockSelection::Earliest => s.rec_str("filter", &"earliest"),
        BlockSelection::Number(number) => s.rec_str("filter", &number),
    });

    // execute
    let block = ctx.storage.read_block(&block_selection)?;

    Span::with(|s| {
        s.record("found", block.is_some());
        if let Some(ref block) = block {
            s.rec_str("number", &block.number());
        }
    });

    // handle response
    match (block, full_transactions) {
        (Some(block), true) => Ok(block.to_json_rpc_with_full_transactions()),
        (Some(block), false) => Ok(block.to_json_rpc_with_transactions_hashes()),
        (None, _) => Ok(JsonValue::Null),
    }
}

#[tracing::instrument(name = "rpc::eth_getUncleByBlockHashAndIndex", skip_all)]
fn eth_get_uncle_by_block_hash_and_index(_: Params<'_>, _: &RpcContext, _: &Extensions) -> anyhow::Result<JsonValue, RpcError> {
    Ok(JsonValue::Null)
}

// -----------------------------------------------------------------------------
// Transaction
// -----------------------------------------------------------------------------

#[tracing::instrument(name = "rpc::eth_getTransactionByHash", skip_all, fields(hash, found))]
fn eth_get_transaction_by_hash(params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> anyhow::Result<JsonValue, RpcError> {
    let (_, hash) = next_rpc_param::<Hash>(params.sequence())?;
    Span::with(|s| s.rec_str("hash", &hash));

    let tx = ctx.storage.read_transaction(&hash)?;
    Span::with(|s| {
        s.record("found", tx.is_some());
    });

    match tx {
        Some(tx) => Ok(tx.to_json_rpc_transaction()),
        None => Ok(JsonValue::Null),
    }
}

#[tracing::instrument(name = "rpc::eth_getTransactionReceipt", skip_all, fields(hash, found))]
fn eth_get_transaction_receipt(params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> anyhow::Result<JsonValue, RpcError> {
    let (_, hash) = next_rpc_param::<Hash>(params.sequence())?;
    Span::with(|s| s.rec_str("hash", &hash));

    let tx = ctx.storage.read_transaction(&hash)?;
    Span::with(|s| {
        s.record("found", tx.is_some());
    });

    match tx {
        Some(mined_transaction) => Ok(mined_transaction.to_json_rpc_receipt()),
        None => Ok(JsonValue::Null),
    }
}

#[tracing::instrument(name = "rpc::eth_estimateGas", skip_all)]
fn eth_estimate_gas(params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> anyhow::Result<String, RpcError> {
    let (_, call) = next_rpc_param::<CallInput>(params.sequence())?;

    match ctx.executor.execute_local_call(call, StoragePointInTime::Present) {
        // result is success
        Ok(result) if result.is_success() => Ok(hex_num(result.gas)),

        // result is failure
        Ok(result) => Err(rpc_internal_error(hex_data(result.output)).into()),

        // internal error
        Err(e) => {
            tracing::error!(reason = ?e, "failed to execute eth_estimateGas");
            Err(error_with_source(e, "failed to execute eth_estimateGas"))
        }
    }
}

#[tracing::instrument(name = "rpc::eth_call", skip_all, fields(from, to))]
fn eth_call(params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> anyhow::Result<String, RpcError> {
    let (params, call) = next_rpc_param::<CallInput>(params.sequence())?;
    let (_, block_selection) = next_rpc_param_or_default::<BlockSelection>(params)?;

    Span::with(|s| {
        s.rec_opt("from", &call.from);
        s.rec_opt("to", &call.to);
    });

    let point_in_time = ctx.storage.translate_to_point_in_time(&block_selection)?;
    match ctx.executor.execute_local_call(call, point_in_time) {
        // success or failure, does not matter
        Ok(result) => Ok(hex_data(result.output)),

        // internal error
        Err(e) => {
            tracing::error!(reason = ?e, "failed to execute eth_call");
            Err(error_with_source(e, "failed to execute eth_call"))
        }
    }
}

#[tracing::instrument(name = "rpc::eth_sendRawTransaction", skip_all, fields(hash, from, to))]
fn eth_send_raw_transaction(params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> anyhow::Result<String, RpcError> {
    let (_, data) = next_rpc_param::<Bytes>(params.sequence())?;
    let tx = parse_rpc_rlp::<TransactionInput>(&data)?;

    Span::with(|s| {
        s.rec_str("hash", &tx.hash);
        s.rec_str("from", &tx.signer);
        s.rec_opt("to", &tx.to);
    });

    // forward transaction to the leader
    // HACK: if importer-online is enabled, we forward the transction to substrate
    if ctx.consensus.should_forward() {
        tracing::info!("forwarding transaction");
        return match Handle::current().block_on(ctx.consensus.forward(tx)) {
            Ok(hash) => Ok(hex_data(hash)),
            Err(e) => {
                tracing::error!(reason = ?e, "failed to forward transaction");
                Err(rpc_internal_error(e.to_string()).into())
            }
        };
    }

    // execute
    let tx_hash = tx.hash;
    match ctx.executor.execute_local_transaction(tx) {
        Ok(_) => Ok(hex_data(tx_hash)),
        Err(e) => {
            tracing::error!(reason = ?e, "failed to execute eth_sendRawTransaction");
            Err(error_with_source(e, "failed to execute eth_sendRawTransaction"))
        }
    }
}

// -----------------------------------------------------------------------------
// Logs
// -----------------------------------------------------------------------------

#[tracing::instrument(name = "rpc::eth_getLogs", skip_all)]
fn eth_get_logs(params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> anyhow::Result<JsonValue, RpcError> {
    const MAX_BLOCK_RANGE: u64 = 5_000;

    let (_, filter_input) = next_rpc_param_or_default::<LogFilterInput>(params.sequence())?;
    let mut filter = filter_input.parse(&ctx.storage)?;

    // for this operation, the filter always need the end block specified to calculate the difference
    if filter.to_block.is_none() {
        filter.to_block = Some(ctx.storage.read_mined_block_number()?);
    }

    // check range
    let blocks_in_range = filter.from_block.count_to(&filter.to_block.unwrap());
    if blocks_in_range > MAX_BLOCK_RANGE {
        return Err(rpc_invalid_params_error(format!(
            "filter range will fetch logs from {} blocks, but the max allowed is {}",
            blocks_in_range, MAX_BLOCK_RANGE
        ))
        .into());
    }

    let logs = ctx.storage.read_logs(&filter)?;
    Ok(JsonValue::Array(logs.into_iter().map(|x| x.to_json_rpc_log()).collect()))
}

// -----------------------------------------------------------------------------
// Account
// -----------------------------------------------------------------------------

#[tracing::instrument(name = "rpc::eth_accounts", skip_all)]
fn eth_accounts(_: Params<'_>, _ctx: &RpcContext, _: &Extensions) -> anyhow::Result<JsonValue, RpcError> {
    Ok(json!([]))
}

#[tracing::instrument(name = "rpc::eth_getTransactionCount", skip_all, fields(address))]
fn eth_get_transaction_count(params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> anyhow::Result<String, RpcError> {
    let (params, address) = next_rpc_param::<Address>(params.sequence())?;
    let (_, block_selection) = next_rpc_param_or_default::<BlockSelection>(params)?;

    Span::with(|s| {
        s.rec_str("address", &address);
    });

    let point_in_time = ctx.storage.translate_to_point_in_time(&block_selection)?;
    let account = ctx.storage.read_account(&address, &point_in_time)?;
    Ok(hex_num(account.nonce))
}

#[tracing::instrument(name = "rpc::eth_getBalance", skip_all, fields(address))]
fn eth_get_balance(params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> anyhow::Result<String, RpcError> {
    let (params, address) = next_rpc_param::<Address>(params.sequence())?;
    let (_, block_selection) = next_rpc_param_or_default::<BlockSelection>(params)?;

    Span::with(|s| {
        s.rec_str("address", &address);
    });

    let point_in_time = ctx.storage.translate_to_point_in_time(&block_selection)?;
    let account = ctx.storage.read_account(&address, &point_in_time)?;

    Ok(hex_num(account.balance))
}

#[tracing::instrument(name = "rpc::eth_getCode", skip_all, fields(address))]
fn eth_get_code(params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> anyhow::Result<String, RpcError> {
    let (params, address) = next_rpc_param::<Address>(params.sequence())?;
    let (_, block_selection) = next_rpc_param_or_default::<BlockSelection>(params)?;

    Span::with(|s| {
        s.rec_str("address", &address);
    });

    let point_in_time = ctx.storage.translate_to_point_in_time(&block_selection)?;
    let account = ctx.storage.read_account(&address, &point_in_time)?;

    Ok(account.bytecode.map(hex_data).unwrap_or_else(hex_null))
}

// -----------------------------------------------------------------------------
// Subscriptions
// -----------------------------------------------------------------------------

#[tracing::instrument(name = "rpc::eth_subscribe", skip_all)]
async fn eth_subscribe(params: Params<'_>, pending: PendingSubscriptionSink, ctx: Arc<RpcContext>, ext: Extensions) -> impl IntoSubscriptionCloseResponse {
    let client = ext.rpc_client();
    let (params, kind) = next_rpc_param::<String>(params.sequence())?;
    match kind.deref() {
        "newPendingTransactions" => {
            ctx.subs.add_new_pending_txs(client, pending.accept().await?).await;
        }

        "newHeads" => {
            ctx.subs.add_new_heads(client, pending.accept().await?).await;
        }

        "logs" => {
            let (_, filter) = next_rpc_param_or_default::<LogFilterInput>(params)?;
            let filter = filter.parse(&ctx.storage)?;
            ctx.subs.add_logs(client, filter, pending.accept().await?).await;
        }

        // unsupported
        kind => {
            tracing::warn!(%kind, "unsupported subscription kind");
            pending
                .reject(rpc_invalid_params_error(format!("unsupported subscription kind: {}", kind)))
                .await;
        }
    };
    Ok(())
}

// -----------------------------------------------------------------------------
// Storage
// -----------------------------------------------------------------------------

#[tracing::instrument(name = "rpc::eth_getStorageAt", skip_all, fields(address, index))]
fn eth_get_storage_at(params: Params<'_>, ctx: Arc<RpcContext>, _: Extensions) -> anyhow::Result<String, RpcError> {
    let (params, address) = next_rpc_param::<Address>(params.sequence())?;
    let (params, index) = next_rpc_param::<SlotIndex>(params)?;
    let (_, block_selection) = next_rpc_param_or_default::<BlockSelection>(params)?;

    Span::with(|s| {
        s.rec_str("address", &address);
        s.rec_str("index", &index);
    });

    let point_in_time = ctx.storage.translate_to_point_in_time(&block_selection)?;
    let slot = ctx.storage.read_slot(&address, &index, &point_in_time)?;

    // It must be padded, even if it is zero.
    Ok(hex_num_zero_padded(slot.value.as_u256()))
}

// -----------------------------------------------------------------------------
// Helpers
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
