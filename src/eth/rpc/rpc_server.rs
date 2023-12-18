//! RPC server for HTTP and WS.

use std::sync::Arc;

use jsonrpsee::server::RpcModule;
use jsonrpsee::server::RpcServiceBuilder;
use jsonrpsee::server::Server;
use jsonrpsee::types::error::PARSE_ERROR_CODE;
use jsonrpsee::types::error::PARSE_ERROR_MSG;
use jsonrpsee::types::ErrorObjectOwned;
use jsonrpsee::types::Params;
use jsonrpsee::types::ParamsSequence;
use rlp::Decodable;
use serde_json::Value as JsonValue;

use crate::eth::miner::BlockMiner;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumberSelection;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::CallInput;
use crate::eth::primitives::Hash;
use crate::eth::primitives::TransactionInput;
use crate::eth::rpc::RpcContext;
use crate::eth::storage::BlockNumberStorage;
use crate::eth::storage::EthStorage;
use crate::eth::EthCall;
use crate::eth::EthDeployment;
use crate::eth::EthExecutor;
use crate::eth::EthTransaction;

// -----------------------------------------------------------------------------
// Server
// -----------------------------------------------------------------------------

const MAX_LOG_LENGTH: u32 = 1024;

pub async fn serve_rpc(executor: EthExecutor, eth_storage: Arc<impl EthStorage>, block_number_storage: Arc<impl BlockNumberStorage>) -> eyre::Result<()> {
    // configure context
    let ctx = RpcContext {
        chain_id: 2008,
        client_version: "ledger",
        gas_price: 0,

        // services
        executor,
        eth_storage,
        block_number_storage,
    };
    tracing::info!(?ctx, "starting rpc server");

    // configure module
    let mut module = RpcModule::<RpcContext>::new(ctx);
    module = register_routes(module)?;

    // serve module
    let rpc_middleware = RpcServiceBuilder::new().rpc_logger(MAX_LOG_LENGTH);
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
    module.register_method("eth_getBlockByNumber", eth_get_block_by_number)?;

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
    let number = ctx.block_number_storage.current_block_number()?;
    Ok(serde_json::to_value(number).unwrap())
}
/// TODO
fn eth_get_block_by_number(params: Params, ctx: &RpcContext) -> Result<JsonValue, ErrorObjectOwned> {
    let (params, number_selection) = parse_param::<BlockNumberSelection>(params.sequence())?;
    let (_, full_transactions) = parse_param::<bool>(params)?;

    // parse transaction
    let number = match number_selection {
        BlockNumberSelection::Latest => ctx.block_number_storage.current_block_number()?,
        BlockNumberSelection::Block(number) => number,
    };

    // handle genesis block
    let block = if number.is_genesis() {
        Some(BlockMiner::genesis())
    } else {
        ctx.eth_storage.read_block(&number)?
    };

    match (block, full_transactions) {
        (Some(block), true) => Ok(block.to_json_with_full_transactions()),
        (Some(block), false) => Ok(block.to_json_with_transactions_hashes()),
        (None, _) => Ok(JsonValue::Null),
    }
}

// Transaction

/// OK
fn eth_get_transaction_count(params: Params, ctx: &RpcContext) -> Result<String, ErrorObjectOwned> {
    let (_, address) = parse_param::<Address>(params.sequence())?;
    let account = ctx.eth_storage.read_account(&address)?;

    Ok(hex_num(account.nonce))
}

/// TODO
fn eth_get_transaction_by_hash(params: Params, ctx: &RpcContext) -> Result<JsonValue, ErrorObjectOwned> {
    let (_, hash) = parse_param::<Hash>(params.sequence())?;
    let mined = ctx.eth_storage.read_mined_transaction(&hash)?;

    match mined {
        Some(mined) => Ok(serde_json::to_value(mined).unwrap()),
        None => Ok(JsonValue::Null),
    }
}

/// TODO
fn eth_get_transaction_receipt(params: Params, ctx: &RpcContext) -> Result<JsonValue, ErrorObjectOwned> {
    let (_, hash) = parse_param::<Hash>(params.sequence())?;
    match ctx.eth_storage.read_mined_transaction(&hash)? {
        Some(mined_transaction) => Ok(serde_json::to_value(&mined_transaction.to_receipt()).unwrap()),
        None => Ok(JsonValue::Null),
    }
}

fn eth_call(params: Params, ctx: &RpcContext) -> Result<String, ErrorObjectOwned> {
    // decode
    let (_, call) = parse_param::<CallInput>(params.sequence())?;

    // execute
    let result = ctx.executor.call(EthCall {
        contract: call.to,
        data: call.data,
    });
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
    let (_, data) = parse_param::<Bytes>(params.sequence())?;
    let transaction = parse_rlp::<TransactionInput>(&data)?;
    let caller = transaction.signer()?;

    // execute
    let hash = transaction.hash.clone();
    let result = match transaction.to.clone() {
        // function call
        Some(contract) => ctx.executor.transact(EthTransaction {
            caller,
            contract,
            data: transaction.input.clone(),
            transaction,
        }),

        // deployment
        None => ctx
            .executor
            .deploy(EthDeployment {
                caller,
                data: transaction.input.clone(),
                transaction,
            })
            .map(|_| ()),
    };

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
    let (_, address) = parse_param::<Address>(params.sequence())?;
    let account = ctx.eth_storage.read_account(&address)?;

    Ok(account.bytecode.map(hex_data).unwrap_or_else(hex_zero))
}

// -----------------------------------------------------------------------------
// Parsers
// -----------------------------------------------------------------------------
fn parse_param<'a, T: serde::Deserialize<'a>>(mut params: ParamsSequence<'a>) -> Result<(ParamsSequence, T), ErrorObjectOwned> {
    match params.next::<T>() {
        Ok(address) => Ok((params, address)),
        Err(e) => {
            tracing::warn!(reason = ?e, kind = std::any::type_name::<T>(), "failed to parse input param");
            Err(e)
        }
    }
}

fn parse_rlp<T: Decodable>(value: &[u8]) -> Result<T, ErrorObjectOwned> {
    match rlp::decode::<T>(value) {
        Ok(trx) => Ok(trx),
        Err(e) => {
            tracing::warn!(reason = ?e, "failed to decode rlp data");
            Err(error_parsing(value))
        }
    }
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

fn error_parsing<S: serde::Serialize>(data: S) -> ErrorObjectOwned {
    ErrorObjectOwned::owned(PARSE_ERROR_CODE, PARSE_ERROR_MSG, Some(data))
}
