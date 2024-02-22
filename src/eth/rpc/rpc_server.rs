//! RPC server for HTTP and WS.

use std::ops::Deref;
use std::sync::Arc;

use ethereum_types::U256;
use jsonrpsee::server::middleware::http::ProxyGetRequestLayer;
use jsonrpsee::server::RandomStringIdProvider;
use jsonrpsee::server::RpcModule;
use jsonrpsee::server::RpcServiceBuilder;
use jsonrpsee::server::Server;
use jsonrpsee::types::Params;
use jsonrpsee::IntoSubscriptionCloseResponse;
use jsonrpsee::PendingSubscriptionSink;
use serde_json::Value as JsonValue;

use crate::config::Environment;
use crate::config::StratusConfig;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::CallInput;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilterInput;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionInput;
use crate::eth::rpc::next_rpc_param;
use crate::eth::rpc::next_rpc_param_or_default;
use crate::eth::rpc::parse_rpc_rlp;
use crate::eth::rpc::rpc_internal_error;
use crate::eth::rpc::rpc_parsing_error;
use crate::eth::rpc::RpcContext;
use crate::eth::rpc::RpcError;
use crate::eth::rpc::RpcMiddleware;
use crate::eth::rpc::RpcSubscriptions;
use crate::eth::storage::StratusStorage;
use crate::eth::EthExecutor;

// -----------------------------------------------------------------------------
// Server
// -----------------------------------------------------------------------------

/// Starts JSON-RPC server.
pub async fn serve_rpc(executor: EthExecutor, storage: Arc<StratusStorage>, config: StratusConfig) -> anyhow::Result<()> {
    // configure subscriptions
    let subs = Arc::new(RpcSubscriptions::default());
    let subscriptions_cleaner_handle = Arc::clone(&subs).spawn_subscriptions_cleaner();
    let logs_notifier_handle = Arc::clone(&subs).spawn_logs_notifier(executor.subscribe_to_logs());
    let heads_notifier_handle = Arc::clone(&subs).spawn_new_heads_notifier(executor.subscribe_to_new_heads());
    let address = config.address;

    // configure context
    let ctx = RpcContext {
        chain_id: 2008,
        client_version: "stratus",
        gas_price: 0,
        env: config.common.env,

        // services
        executor,
        storage,

        // subscriptions
        subs,
    };
    tracing::info!(%address, ?ctx, "starting rpc server");

    // configure module
    let env = ctx.env;
    let mut module = RpcModule::<RpcContext>::new(ctx);
    module = register_methods(module, env)?;

    // configure middleware
    let rpc_middleware = RpcServiceBuilder::new().layer_fn(RpcMiddleware::new);
    let http_middleware = tower::ServiceBuilder::new().layer(ProxyGetRequestLayer::new("/health", "net_listening").unwrap());

    // serve module
    let server = Server::builder()
        .set_rpc_middleware(rpc_middleware)
        .set_http_middleware(http_middleware)
        .set_id_provider(RandomStringIdProvider::new(8))
        .build(address)
        .await?;
    let _server_handle = server.start(module);

    tokio::select! {
        _ = subscriptions_cleaner_handle => {
            tracing::info!("subscriptions cleaner stopped");
        }
        _ = logs_notifier_handle => {
            tracing::info!("logs notifier stopped");
        }
        _ = heads_notifier_handle => {
            tracing::info!("heads notifier stopped");
        }
    }

    Ok(())
}

fn register_methods(mut module: RpcModule<RpcContext>, env: Environment) -> anyhow::Result<RpcModule<RpcContext>> {
    // debug
    #[cfg(feature = "evm-set-timestamp")]
    module.register_async_method("evm_setNextBlockTimestamp", evm_set_next_block_timestamp)?;
    #[cfg(feature = "evm-mine")]
    module.register_async_method("evm_mine", evm_mine)?;

    if env.is_development() {
        module.register_async_method("debug_setHead", debug_set_head)?;
    }

    // blockchain
    module.register_async_method("net_version", net_version)?;
    module.register_async_method("net_listening", net_listening)?;
    module.register_async_method("eth_chainId", eth_chain_id)?;
    module.register_async_method("web3_clientVersion", web3_client_version)?;

    // gas
    module.register_async_method("eth_gasPrice", eth_gas_price)?;

    // block
    module.register_async_method("eth_blockNumber", eth_block_number)?;
    module.register_async_method("eth_getBlockByNumber", eth_get_block_by_selector)?;
    module.register_async_method("eth_getBlockByHash", eth_get_block_by_selector)?;
    module.register_async_method("eth_getUncleByBlockHashAndIndex", eth_get_uncle_by_block_hash_and_index)?;

    // transactions
    module.register_async_method("eth_getTransactionCount", eth_get_transaction_count)?;
    module.register_async_method("eth_getTransactionByHash", eth_get_transaction_by_hash)?;
    module.register_async_method("eth_getTransactionReceipt", eth_get_transaction_receipt)?;
    module.register_async_method("eth_estimateGas", eth_estimate_gas)?;
    module.register_async_method("eth_call", eth_call)?;
    module.register_async_method("eth_sendRawTransaction", eth_send_raw_transaction)?;

    // logs
    module.register_async_method("eth_getLogs", eth_get_logs)?;

    // account
    module.register_async_method("eth_getBalance", eth_get_balance)?;
    module.register_async_method("eth_getCode", eth_get_code)?;

    // subscriptions
    module.register_subscription("eth_subscribe", "eth_subscription", "eth_unsubscribe", eth_subscribe)?;

    // storage
    module.register_async_method("eth_getStorageAt", eth_get_storage_at)?;

    Ok(module)
}

// -----------------------------------------------------------------------------
// Handlers
// -----------------------------------------------------------------------------

// Debug
async fn debug_set_head(params: Params<'_>, ctx: Arc<RpcContext>) -> anyhow::Result<JsonValue, RpcError> {
    let (_, number) = next_rpc_param::<BlockNumber>(params.sequence())?;
    ctx.storage.reset_temp().await?;
    ctx.storage.reset_perm(number).await?;
    Ok(serde_json::to_value(number).unwrap())
}

#[cfg(feature = "evm-mine")]
async fn evm_mine(_params: Params<'_>, ctx: Arc<RpcContext>) -> anyhow::Result<JsonValue, RpcError> {
    ctx.executor.mine_empty_block().await?;
    Ok(serde_json::to_value(true).unwrap())
}

#[cfg(feature = "evm-set-timestamp")]
async fn evm_set_next_block_timestamp(params: Params<'_>, ctx: Arc<RpcContext>) -> anyhow::Result<JsonValue, RpcError> {
    use crate::eth::primitives::UnixTime;
    use crate::log_and_err;

    let (_, timestamp) = next_rpc_param::<UnixTime>(params.sequence())?;
    let latest = ctx.storage.read_block(&BlockSelection::Latest).await?;
    match latest {
        Some(block) => UnixTime::set_offset(timestamp, block.header.timestamp)?,
        None => return log_and_err!("reading latest block returned None")?,
    }
    Ok(serde_json::to_value(timestamp).unwrap())
}

// Status
async fn net_listening(_: Params<'_>, _: Arc<RpcContext>) -> bool {
    true
}

// Blockchain

async fn net_version(_: Params<'_>, ctx: Arc<RpcContext>) -> String {
    ctx.chain_id.to_string()
}

async fn eth_chain_id(_: Params<'_>, ctx: Arc<RpcContext>) -> String {
    hex_num(ctx.chain_id)
}

async fn web3_client_version(_: Params<'_>, ctx: Arc<RpcContext>) -> String {
    ctx.client_version.to_owned()
}

// Gas

async fn eth_gas_price(_: Params<'_>, _: Arc<RpcContext>) -> String {
    hex_zero()
}

// Block
async fn eth_block_number(_params: Params<'_>, ctx: Arc<RpcContext>) -> anyhow::Result<JsonValue, RpcError> {
    let number = ctx.storage.read_current_block_number().await?;
    Ok(serde_json::to_value(number).unwrap())
}

async fn eth_get_block_by_selector(params: Params<'_>, ctx: Arc<RpcContext>) -> anyhow::Result<JsonValue, RpcError> {
    let (params, block_selection) = next_rpc_param::<BlockSelection>(params.sequence())?;
    let (_, full_transactions) = next_rpc_param::<bool>(params)?;

    let block = ctx.storage.read_block(&block_selection).await?;

    match (block, full_transactions) {
        (Some(block), true) => Ok(block.to_json_rpc_with_full_transactions()),
        (Some(block), false) => Ok(block.to_json_rpc_with_transactions_hashes()),
        (None, _) => Ok(JsonValue::Null),
    }
}

async fn eth_get_uncle_by_block_hash_and_index(_params: Params<'_>, _ctx: Arc<RpcContext>) -> anyhow::Result<JsonValue, RpcError> {
    Ok(JsonValue::Null)
}

// Transaction

async fn eth_get_transaction_count(params: Params<'_>, ctx: Arc<RpcContext>) -> anyhow::Result<String, RpcError> {
    let (params, address) = next_rpc_param::<Address>(params.sequence())?;
    let (_, block_selection) = next_rpc_param_or_default::<BlockSelection>(params)?;

    let point_in_time = ctx.storage.translate_to_point_in_time(&block_selection).await?;
    let account = ctx.storage.read_account(&address, &point_in_time).await?;
    Ok(hex_num(account.nonce))
}

async fn eth_get_transaction_by_hash(params: Params<'_>, ctx: Arc<RpcContext>) -> anyhow::Result<JsonValue, RpcError> {
    let (_, hash) = next_rpc_param::<Hash>(params.sequence())?;
    let mined = ctx.storage.read_mined_transaction(&hash).await?;

    match mined {
        Some(mined) => Ok(mined.to_json_rpc_transaction()),
        None => Ok(JsonValue::Null),
    }
}

async fn eth_get_transaction_receipt(params: Params<'_>, ctx: Arc<RpcContext>) -> anyhow::Result<JsonValue, RpcError> {
    let (_, hash) = next_rpc_param::<Hash>(params.sequence())?;
    match ctx.storage.read_mined_transaction(&hash).await? {
        Some(mined_transaction) => Ok(mined_transaction.to_json_rpc_receipt()),
        None => Ok(JsonValue::Null),
    }
}

async fn eth_estimate_gas(params: Params<'_>, ctx: Arc<RpcContext>) -> anyhow::Result<String, RpcError> {
    let (_, call) = next_rpc_param::<CallInput>(params.sequence())?;

    match ctx.executor.call(call, StoragePointInTime::Present).await {
        // result is success
        Ok(result) if result.is_success() => Ok(hex_num(result.gas)),

        // result is failure
        Ok(result) => Err(RpcError::Response(rpc_internal_error(hex_data(result.output)))),

        // internal error
        Err(e) => {
            tracing::error!(reason = ?e, "failed to execute eth_estimateGas");
            Err(e.into())
        }
    }
}

async fn eth_call(params: Params<'_>, ctx: Arc<RpcContext>) -> anyhow::Result<String, RpcError> {
    let (params, call) = next_rpc_param::<CallInput>(params.sequence())?;
    let (_, block_selection) = next_rpc_param_or_default::<BlockSelection>(params)?;

    let point_in_time = ctx.storage.translate_to_point_in_time(&block_selection).await?;
    match ctx.executor.call(call, point_in_time).await {
        // success or failure, does not matter
        Ok(result) => Ok(hex_data(result.output)),

        // internal error
        Err(e) => {
            tracing::error!(reason = ?e, "failed to execute eth_call");
            Err(e.context("failed to execute eth_call").into())
        }
    }
}

async fn eth_send_raw_transaction(params: Params<'_>, ctx: Arc<RpcContext>) -> anyhow::Result<String, RpcError> {
    let (_, data) = next_rpc_param::<Bytes>(params.sequence())?;
    let transaction = parse_rpc_rlp::<TransactionInput>(&data)?;

    let hash = transaction.hash.clone();
    match ctx.executor.transact(transaction).await {
        // result is success
        Ok(result) if result.is_success() => Ok(hex_data(hash)),

        // result is failure
        Ok(result) => Err(RpcError::Response(rpc_internal_error(hex_data(result.output)))),

        // internal error
        Err(e) => {
            tracing::error!(reason = ?e, "failed to execute eth_sendRawTransaction");
            Err(e.context("failed to execute eth_sendRawTransaction").into())
        }
    }
}

// Logs
async fn eth_get_logs(params: Params<'_>, ctx: Arc<RpcContext>) -> anyhow::Result<JsonValue, RpcError> {
    let (_, filter_input) = next_rpc_param::<LogFilterInput>(params.sequence())?;
    let filter = filter_input.parse(&ctx.storage).await?;

    let logs = ctx.storage.read_logs(&filter).await?;
    Ok(JsonValue::Array(logs.into_iter().map(|x| x.to_json_rpc_log()).collect()))
}

// Account

async fn eth_get_balance(params: Params<'_>, ctx: Arc<RpcContext>) -> anyhow::Result<String, RpcError> {
    let (params, address) = next_rpc_param::<Address>(params.sequence())?;
    let (_, block_selection) = next_rpc_param_or_default::<BlockSelection>(params)?;

    let point_in_time = ctx.storage.translate_to_point_in_time(&block_selection).await?;
    let account = ctx.storage.read_account(&address, &point_in_time).await?;

    Ok(hex_num(account.balance))
}

async fn eth_get_code(params: Params<'_>, ctx: Arc<RpcContext>) -> anyhow::Result<String, RpcError> {
    let (params, address) = next_rpc_param::<Address>(params.sequence())?;
    let (_, block_selection) = next_rpc_param_or_default::<BlockSelection>(params)?;

    let point_in_time = ctx.storage.translate_to_point_in_time(&block_selection).await?;
    let account = ctx.storage.read_account(&address, &point_in_time).await?;

    Ok(account.bytecode.map(hex_data).unwrap_or_else(hex_zero))
}

// Subscription

async fn eth_subscribe(params: Params<'_>, pending: PendingSubscriptionSink, ctx: Arc<RpcContext>) -> impl IntoSubscriptionCloseResponse {
    let (params, kind) = next_rpc_param::<String>(params.sequence())?;
    match kind.deref() {
        // new block emitted
        "newHeads" => {
            ctx.subs.add_new_heads(pending.accept().await?).await;
        }

        // transaction logs emitted
        "logs" => {
            let (_, filter) = next_rpc_param_or_default::<LogFilterInput>(params)?;
            let filter = filter.parse(&ctx.storage).await?;
            ctx.subs.add_logs(pending.accept().await?, filter).await;
        }

        // unsupported
        kind => {
            tracing::warn!(%kind, "unsupported subscription kind");
            pending.reject(rpc_parsing_error(format!("unsupported subscription kind: {}", kind))).await;
        }
    };
    Ok(())
}

// Storage
async fn eth_get_storage_at(params: Params<'_>, ctx: Arc<RpcContext>) -> anyhow::Result<String, RpcError> {
    let (params, address) = next_rpc_param::<Address>(params.sequence())?;
    let (params, index) = next_rpc_param::<SlotIndex>(params)?;
    let (_, block_selection) = next_rpc_param_or_default::<BlockSelection>(params)?;

    let point_in_time = ctx.storage.translate_to_point_in_time(&block_selection).await?;
    let slot = ctx.storage.read_slot(&address, &index, &point_in_time).await?;

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

fn hex_zero() -> String {
    "0x0".to_owned()
}
