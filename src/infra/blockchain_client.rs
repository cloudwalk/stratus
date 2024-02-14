use std::time::Duration;

use jsonrpsee::core::client::ClientT;
use jsonrpsee::http_client::HttpClient;
use jsonrpsee::http_client::HttpClientBuilder;
use serde_json::Value as JsonValue;

use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Wei;
use crate::log_and_err;

pub struct BlockchainClient {
    http: HttpClient,
}

impl BlockchainClient {
    /// Creates a new RPC client.
    pub fn new(url: &str, timeout: Duration) -> anyhow::Result<Self> {
        tracing::info!(%url, "starting blockchain client");

        match HttpClientBuilder::default().request_timeout(timeout).build(url) {
            Ok(http) => Ok(Self { http }),
            Err(e) => log_and_err!(reason = e, "failed to create blockchain http client"),
        }
    }

    /// Retrieves the current block number.
    pub async fn get_current_block_number(&self) -> anyhow::Result<BlockNumber> {
        tracing::debug!("retrieving current block number");
        match self.http.request::<BlockNumber, Vec<()>>("eth_blockNumber", vec![]).await {
            Ok(number) => Ok(number),
            Err(e) => log_and_err!(reason = e, "failed to retrieve current block number"),
        }
    }

    /// Retrieves a block by number.
    pub async fn get_block_by_number(&self, number: BlockNumber) -> anyhow::Result<JsonValue> {
        tracing::debug!(%number, "retrieving block");

        let number = serde_json::to_value(number)?;
        let result = self
            .http
            .request::<JsonValue, Vec<JsonValue>>("eth_getBlockByNumber", vec![number, JsonValue::Bool(true)])
            .await;

        match result {
            Ok(block) => Ok(block),
            Err(e) => log_and_err!(reason = e, "failed to retrieve block by number"),
        }
    }

    /// Retrieves a transaction receipt.
    pub async fn get_transaction_receipt(&self, hash: &Hash) -> anyhow::Result<JsonValue> {
        tracing::debug!(%hash, "retrieving transaction receipt");

        let hash = serde_json::to_value(hash)?;
        let result = self.http.request::<JsonValue, Vec<JsonValue>>("eth_getTransactionReceipt", vec![hash]).await;

        match result {
            Ok(receipt) => Ok(receipt),
            Err(e) => log_and_err!(reason = e, "failed to retrieve transaction receipt by hash"),
        }
    }

    /// Retrieves an account balance at some block.
    pub async fn get_balance(&self, address: &Address, number: Option<BlockNumber>) -> anyhow::Result<Wei> {
        tracing::debug!(%address, ?number, "retrieving account balance");

        let address = serde_json::to_value(address)?;
        let number = serde_json::to_value(number)?;
        let result = self.http.request::<Wei, Vec<JsonValue>>("eth_getBalance", vec![address, number]).await;

        match result {
            Ok(receipt) => Ok(receipt),
            Err(e) => log_and_err!(reason = e, "failed to retrieve account balance"),
        }
    }
}
