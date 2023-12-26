//! RPC server for HTTP and WS.

use std::sync::Arc;

use jsonrpsee::server::RpcModule;
use jsonrpsee::server::RpcServiceBuilder;
use jsonrpsee::server::Server;
use jsonrpsee::types::ErrorObjectOwned;
use jsonrpsee::types::Params;
use serde_json::Value as JsonValue;

use crate::eth::primitives::Address;
use crate::eth::primitives::BlockSelection;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::CallInput;
use crate::eth::primitives::Hash;
use crate::eth::primitives::TransactionInput;
use crate::eth::rpc::next_rpc_param;
use crate::eth::rpc::parse_rpc_rlp;
use crate::eth::rpc::RpcContext;
use crate::eth::rpc::RpcMiddleware;
use crate::eth::storage::EthStorage;
use crate::eth::EthExecutor;

// -----------------------------------------------------------------------------
// Server
// -----------------------------------------------------------------------------

pub async fn serve_rpc(executor: EthExecutor, eth_storage: Arc<dyn EthStorage>) -> eyre::Result<()> {
    // configure context
    let ctx = RpcContext {
        chain_id: 2008,
        client_version: "ledger",
        gas_price: 0,

        // services
        executor,
        storage: eth_storage,
    };
    tracing::info!(?ctx, "starting rpc server");

    // configure module
    let mut module = RpcModule::<RpcContext>::new(ctx);
    module = register_routes(module)?;

    // configure middleware
    let rpc_middleware = RpcServiceBuilder::new().layer_fn(RpcMiddleware::new);

    // serve module
    let server = Server::builder().set_rpc_middleware(rpc_middleware).build("0.0.0.0:3000").await?;
    let handle = server.start(module);
    handle.stopped().await;

    Ok(())
}

fn register_routes(mut module: RpcModule<RpcContext>) -> eyre::Result<RpcModule<RpcContext>> {
    // blockchain
    module.register_method("net_version", net_version)?;
    module.register_method("eth_chainId", eth_chain_id)?;
    module.register_method("web3_clientVersion", web3_client_version)?;

    // gas
    module.register_method("eth_gasPrice", eth_gas_price)?;
    module.register_method("eth_estimateGas", eth_estimate_gas)?;

    // block
    module.register_method("eth_blockNumber", eth_block_number)?;
    module.register_method("eth_getBlockByNumber", eth_get_block_by_selector)?;
    module.register_method("eth_getBlockByHash", eth_get_block_by_selector)?;

    // transactions
    module.register_method("eth_getTransactionCount", eth_get_transaction_count)?;
    module.register_method("eth_getTransactionByHash", eth_get_transaction_by_hash)?;
    module.register_method("eth_getTransactionReceipt", eth_get_transaction_receipt)?;
    module.register_method("eth_call", eth_call)?;
    module.register_method("eth_sendRawTransaction", eth_send_raw_transaction)?;

    // contract
    module.register_method("eth_getCode", eth_get_code)?;

    Ok(module)
}

// -----------------------------------------------------------------------------
// Handlers
// -----------------------------------------------------------------------------

// Blockchain

/// OK
fn net_version(_: Params, ctx: &RpcContext) -> String {
    ctx.chain_id.to_string()
}

/// OK
fn eth_chain_id(_: Params, ctx: &RpcContext) -> String {
    hex_num(ctx.chain_id)
}

/// OK
fn web3_client_version(_: Params, ctx: &RpcContext) -> String {
    ctx.client_version.to_owned()
}

// Gas

/// OK
fn eth_gas_price(_: Params, _: &RpcContext) -> String {
    hex_zero()
}

/// OK
fn eth_estimate_gas(_: Params, _: &RpcContext) -> String {
    hex_zero()
}

// Block

fn eth_block_number(_: Params, ctx: &RpcContext) -> Result<JsonValue, ErrorObjectOwned> {
    let number = ctx.storage.read_current_block_number()?;
    Ok(serde_json::to_value(number).unwrap())
}
fn eth_get_block_by_selector(params: Params, ctx: &RpcContext) -> Result<JsonValue, ErrorObjectOwned> {
    let (params, block_selection) = next_rpc_param::<BlockSelection>(params.sequence())?;
    let (_, full_transactions) = next_rpc_param::<bool>(params)?;

    let block = ctx.storage.read_block(&block_selection)?;

    match (block, full_transactions) {
        (Some(block), true) => Ok(block.to_json_with_full_transactions()),
        (Some(block), false) => Ok(block.to_json_with_transactions_hashes()),
        (None, _) => Ok(JsonValue::Null),
    }
}

// Transaction

/// OK
fn eth_get_transaction_count(params: Params, ctx: &RpcContext) -> Result<String, ErrorObjectOwned> {
    let (_, address) = next_rpc_param::<Address>(params.sequence())?;
    let account = ctx.storage.read_account(&address)?;

    Ok(hex_num(account.nonce))
}

fn eth_get_transaction_by_hash(params: Params, ctx: &RpcContext) -> Result<JsonValue, ErrorObjectOwned> {
    let (_, hash) = next_rpc_param::<Hash>(params.sequence())?;
    let mined = ctx.storage.read_mined_transaction(&hash)?;

    match mined {
        Some(mined) => Ok(serde_json::to_value(mined).unwrap()),
        None => Ok(JsonValue::Null),
    }
}

fn eth_get_transaction_receipt(params: Params, ctx: &RpcContext) -> Result<JsonValue, ErrorObjectOwned> {
    let (_, hash) = next_rpc_param::<Hash>(params.sequence())?;
    match ctx.storage.read_mined_transaction(&hash)? {
        Some(mined_transaction) => Ok(serde_json::to_value(&mined_transaction.to_receipt()).unwrap()),
        None => Ok(JsonValue::Null),
    }
}

fn eth_call(params: Params, ctx: &RpcContext) -> Result<String, ErrorObjectOwned> {
    // decode
    let (params, call) = next_rpc_param::<CallInput>(params.sequence())?;
    let block_selection = next_rpc_param::<Option<BlockSelection>>(params)?.1.unwrap_or_default();

    // execute
    let block_number = ctx.storage.translate_to_point_in_time(&block_selection)?;
    let result = ctx.executor.call(call, block_number);
    match result {
        Ok(output) => Ok(hex_data(output)),
        Err(e) => {
            tracing::error!(reason = ?e, "failed to execute eth_call");
            Err(e.into())
        }
    }
}

fn eth_send_raw_transaction(params: Params, ctx: &RpcContext) -> Result<String, ErrorObjectOwned> {
    // decode
    let (_, data) = next_rpc_param::<Bytes>(params.sequence())?;
    let transaction = parse_rpc_rlp::<TransactionInput>(&data)?;

    // execute
    let hash = transaction.hash.clone();
    let result = ctx.executor.transact(transaction);

    match result {
        Ok(_) => Ok(hex_data(hash)),
        Err(e) => {
            tracing::error!(reason = ?e, "failed to execute eth_sendRawTransaction");
            Err(e.into())
        }
    }
}

// Code

/// OK
fn eth_get_code(params: Params, ctx: &RpcContext) -> Result<String, ErrorObjectOwned> {
    let (_, address) = next_rpc_param::<Address>(params.sequence())?;
    let account = ctx.storage.read_account(&address)?;

    Ok(account.bytecode.map(hex_data).unwrap_or_else(hex_zero))
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------
#[inline(always)]
fn hex_data<T: AsRef<[u8]>>(value: T) -> String {
    const_hex::encode_prefixed(value)
}

#[inline(always)]
fn hex_num(value: impl Into<usize>) -> String {
    format!("{:#x}", value.into())
}

fn hex_zero() -> String {
    "0x0".to_owned()
}
