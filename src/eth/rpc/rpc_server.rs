//! RPC server for HTTP and WS

use std::sync::Arc;
use std::sync::Mutex;

use ethers_core::types::Block;
use ethers_core::types::Transaction;
use ethers_core::types::TransactionReceipt;
use jsonrpsee::server::RpcModule;
use jsonrpsee::server::Server;
use jsonrpsee::types::error::PARSE_ERROR_CODE;
use jsonrpsee::types::error::PARSE_ERROR_MSG;
use jsonrpsee::types::ErrorObjectOwned;
use jsonrpsee::types::Params;
use jsonrpsee::types::ParamsSequence;
use rlp::Decodable;
use serde_json::Value as JsonValue;

use crate::eth::evm::Evm;
use crate::eth::evm::EvmDeployment;
use crate::eth::evm::EvmStorage;
use crate::eth::evm::EvmTransaction;
use crate::eth::primitives::Address;
use crate::eth::rpc::RpcContext;
use crate::eth::rpc::RpcLogger;

// -----------------------------------------------------------------------------
// Server
// -----------------------------------------------------------------------------
pub async fn serve_rpc(evm: Box<Mutex<impl Evm>>, evm_storage: Arc<impl EvmStorage>) -> eyre::Result<()> {
    // configure context
    let ctx = RpcContext {
        chain_id: 2008,
        client_version: "ledger",
        gas_price: 0,

        // services
        evm,
        evm_storage,
    };
    tracing::info!(?ctx, "starting rpc server");

    // configure module
    let mut module = RpcModule::<RpcContext>::new(ctx);
    module = register_routes(module)?;

    // serve module
    let server = Server::builder().set_logger(RpcLogger).build("0.0.0.0:3000").await?;
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

/// TODO
fn eth_block_number(_: Params, _: &RpcContext) -> String {
    hex_zero()
}

/// TODO
fn eth_get_block_by_number(_: Params, _: &RpcContext) -> JsonValue {
    let block = Block::<String>::default();
    serde_json::to_value(block).unwrap()
}

// Transaction

/// OK
fn eth_get_transaction_count(params: Params, ctx: &RpcContext) -> Result<String, ErrorObjectOwned> {
    // extract
    let (_, address) = parse_address(params.sequence())?;
    let account = ctx.evm_storage.read_account(&address)?;

    Ok(hex_num(account.nonce))
}

/// TODO
fn eth_get_transaction_by_hash(_: Params, _: &RpcContext) -> JsonValue {
    let trx = Transaction::default();
    serde_json::to_value(trx).unwrap()
}

/// TODO
fn eth_get_transaction_receipt(_: Params, _: &RpcContext) -> JsonValue {
    let receipt = TransactionReceipt {
        status: Some(1.into()),
        ..Default::default()
    };
    serde_json::to_value(receipt).unwrap()
}

/// TODO
fn eth_send_raw_transaction(params: Params, ctx: &RpcContext) -> Result<String, ErrorObjectOwned> {
    // decode data
    let (_, data) = parse_hex(params.sequence())?;
    let trx = parse_rlp::<Transaction>(&data)?;
    let trx_signer: Address = match trx.recover_from() {
        Ok(signer) => signer.into(),
        Err(e) => {
            tracing::warn!(reason = ?e, "failed to recover transaction signer");
            return Err(parser_error(Some("cannot recover signer")));
        }
    };

    // execute transaction
    let mut evm = ctx.evm.lock().unwrap();
    match trx.to {
        // function call
        Some(contract) => {
            evm.transact(EvmTransaction {
                caller: trx_signer,
                contract: contract.into(),
                data: trx.input.to_vec(),
            })?;
            Ok(hex_data(trx.hash))
        }

        // deployment
        None => {
            evm.deploy(EvmDeployment {
                caller: trx_signer,
                data: trx.input.into(),
            })?;
            Ok(hex_data(trx.hash))
        }
    }
}

// Code

/// OK
fn eth_get_code(params: Params, ctx: &RpcContext) -> Result<String, ErrorObjectOwned> {
    let (_, address) = parse_address(params.sequence())?;
    let account = ctx.evm_storage.read_account(&address)?;

    Ok(account.bytecode.map(hex_data).unwrap_or_else(hex_zero))
}

// -----------------------------------------------------------------------------
// Parsers
// -----------------------------------------------------------------------------
fn parse_address(mut params: ParamsSequence) -> Result<(ParamsSequence, Address), ErrorObjectOwned> {
    match params.next::<Address>() {
        Ok(address) => Ok((params, address)),
        Err(e) => {
            tracing::warn!(reason = ?e, "failed to parse address");
            Err(e)
        }
    }
}

fn parse_hex(mut params: ParamsSequence) -> Result<(ParamsSequence, Vec<u8>), ErrorObjectOwned> {
    let value = match params.next::<String>() {
        Ok(value) => value,
        Err(e) => {
            tracing::warn!(reason = ?e, "failed to parse hex data");
            return Err(e);
        }
    };
    match const_hex::decode(&value) {
        Ok(value) => Ok((params, value)),
        Err(e) => {
            tracing::warn!(reason = ?e, "failed to parse hex data");
            Err(parser_error(Some(value)))
        }
    }
}

fn parse_rlp<T: Decodable>(value: &[u8]) -> Result<T, ErrorObjectOwned> {
    match rlp::decode::<T>(value) {
        Ok(trx) => Ok(trx),
        Err(e) => {
            tracing::warn!(reason = ?e, "failed to decode rlp data");
            Err(parser_error(Some(value)))
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

fn parser_error<S: serde::Serialize>(data: Option<S>) -> ErrorObjectOwned {
    ErrorObjectOwned::owned(PARSE_ERROR_CODE, PARSE_ERROR_MSG, data)
}
