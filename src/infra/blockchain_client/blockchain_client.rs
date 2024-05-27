use std::time::Duration;

use anyhow::Context;
use ethers_core::types::Bytes;
use ethers_core::types::Transaction;
use jsonrpsee::core::client::ClientT;
use jsonrpsee::core::client::Subscription;
use jsonrpsee::core::client::SubscriptionClientT;
use jsonrpsee::http_client::HttpClient;
use jsonrpsee::http_client::HttpClientBuilder;
use jsonrpsee::ws_client::WsClient;
use jsonrpsee::ws_client::WsClientBuilder;
use serde_json::Value as JsonValue;

use super::pending_transaction::PendingTransaction;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::Hash;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotValue;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::Wei;
use crate::log_and_err;

#[derive(Debug)]
pub struct BlockchainClient {
    http: HttpClient,
    pub http_url: String,
    ws: Option<WsClient>,
    pub ws_url: Option<String>,
}

impl BlockchainClient {
    /// Creates a new RPC client connected only to HTTP.
    pub async fn new_http(http_url: &str, timeout: Duration) -> anyhow::Result<Self> {
        Self::new_http_ws(http_url, None, timeout).await
    }

    /// Creates a new RPC client connected to HTTP and optionally to WS.
    pub async fn new_http_ws(http_url: &str, ws_url: Option<&str>, timeout: Duration) -> anyhow::Result<Self> {
        tracing::info!(%http_url, "starting blockchain client");

        // build http provider
        let http = match HttpClientBuilder::default().request_timeout(timeout).build(http_url) {
            Ok(http) => http,
            Err(e) => {
                tracing::error!(reason = ?e, url = %http_url, "failed to create blockchain http client");
                return Err(e).context("failed to create blockchain http client");
            }
        };

        // build ws provider
        let (ws, ws_url) = if let Some(ws_url) = ws_url {
            match WsClientBuilder::new().connection_timeout(timeout).build(ws_url).await {
                Ok(ws) => (Some(ws), Some(ws_url.to_string())),
                Err(e) => {
                    tracing::error!(reason = ?e, url = %ws_url, "failed to create blockchain websocket client");
                    return Err(e).context("failed to create blockchain websocket client");
                }
            }
        } else {
            (None, None)
        };

        let client = Self {
            http,
            http_url: http_url.to_string(),
            ws,
            ws_url,
        };

        // check health before assuming it is ok
        client.fetch_listening().await?;

        Ok(client)
    }

    // -------------------------------------------------------------------------
    // Websocket
    // -------------------------------------------------------------------------

    /// Checks if the supports websocket connection.
    pub fn supports_ws(&self) -> bool {
        self.ws.is_some()
    }

    /// Validates it is connected to websocket and returns a reference to the websocket client.
    fn require_ws(&self) -> anyhow::Result<&WsClient> {
        match &self.ws {
            Some(ws) => Ok(ws),
            None => log_and_err!("blockchain client not connected to websocket"),
        }
    }

    // -------------------------------------------------------------------------
    // RPC queries
    // -------------------------------------------------------------------------

    /// Checks if the blockchain is listening.
    pub async fn fetch_listening(&self) -> anyhow::Result<()> {
        tracing::debug!("fetching listening status");

        let result = self.http.request::<bool, Vec<()>>("net_listening", vec![]).await;
        match result {
            Ok(_) => Ok(()),
            Err(e) => log_and_err!(reason = e, "failed to fetch listening status"),
        }
    }

    /// Retrieves the current block number.
    pub async fn fetch_block_number(&self) -> anyhow::Result<BlockNumber> {
        tracing::debug!("fetching block number");

        let result = self.http.request::<BlockNumber, Vec<()>>("eth_blockNumber", vec![]).await;

        match result {
            Ok(number) => Ok(number),
            Err(e) => log_and_err!(reason = e, "failed to fetch current block number"),
        }
    }

    /// Fetches a block by number.
    pub async fn fetch_block(&self, number: BlockNumber) -> anyhow::Result<JsonValue> {
        tracing::debug!(%number, "fetching block");

        let number = serde_json::to_value(number)?;
        let result = self
            .http
            .request::<JsonValue, Vec<JsonValue>>("eth_getBlockByNumber", vec![number, JsonValue::Bool(true)])
            .await;

        match result {
            Ok(block) => Ok(block),
            Err(e) => log_and_err!(reason = e, "failed to fetch block by number"),
        }
    }

    /// Fetches a transaction by hash.
    pub async fn fetch_transaction(&self, hash: Hash) -> anyhow::Result<Option<Transaction>> {
        tracing::debug!(%hash, "fetching transaction");

        let hash = serde_json::to_value(hash)?;

        let result = self
            .http
            .request::<Option<Transaction>, Vec<JsonValue>>("eth_getTransactionByHash", vec![hash])
            .await;

        match result {
            Ok(tx) => Ok(tx),
            Err(e) => log_and_err!(reason = e, "failed to fetch transaction by hash"),
        }
    }

    /// Fetches a receipt by hash.
    pub async fn fetch_receipt(&self, hash: Hash) -> anyhow::Result<Option<ExternalReceipt>> {
        tracing::debug!(%hash, "fetching transaction receipt");

        let hash = serde_json::to_value(hash)?;
        let result = self
            .http
            .request::<Option<ExternalReceipt>, Vec<JsonValue>>("eth_getTransactionReceipt", vec![hash])
            .await;

        match result {
            Ok(receipt) => Ok(receipt),
            Err(e) => log_and_err!(reason = e, "failed to fetch transaction receipt by hash"),
        }
    }

    /// Fetches account balance by address and block number.
    pub async fn fetch_balance(&self, address: &Address, number: Option<BlockNumber>) -> anyhow::Result<Wei> {
        tracing::debug!(%address, ?number, "fetching account balance");

        let address = serde_json::to_value(address)?;
        let number = serde_json::to_value(number)?;
        let result = self.http.request::<Wei, Vec<JsonValue>>("eth_getBalance", vec![address, number]).await;

        match result {
            Ok(receipt) => Ok(receipt),
            Err(e) => log_and_err!(reason = e, "failed to fetch account balance"),
        }
    }

    /// Fetches a slot by a slot at some block.
    pub async fn fetch_storage_at(&self, address: &Address, index: &SlotIndex, point_in_time: StoragePointInTime) -> anyhow::Result<SlotValue> {
        tracing::debug!(%address, ?point_in_time, "fetching account balance");

        let address = serde_json::to_value(address)?;
        let index = serde_json::to_value(index)?;
        let number = match point_in_time {
            StoragePointInTime::Present => serde_json::to_value("latest")?,
            StoragePointInTime::Past(number) => serde_json::to_value(number)?,
        };
        let result = self
            .http
            .request::<SlotValue, Vec<JsonValue>>("eth_getStorageAt", vec![address, index, number])
            .await;

        match result {
            Ok(value) => Ok(value),
            Err(e) => log_and_err!(reason = e, "failed to fetch account balance"),
        }
    }

    // -------------------------------------------------------------------------
    // RPC mutations
    // -------------------------------------------------------------------------

    /// Sends a signed transaction.
    pub async fn send_raw_transaction(&self, hash: Hash, tx: Bytes) -> anyhow::Result<PendingTransaction<'_>> {
        tracing::debug!(%hash, "sending raw transaction");

        let tx = serde_json::to_value(tx)?;
        let result = self.http.request::<Hash, Vec<JsonValue>>("eth_sendRawTransaction", vec![tx]).await;

        match result {
            Ok(hash) => Ok(PendingTransaction::new(hash, self)),
            Err(e) => log_and_err!(reason = e, "failed to send raw transaction"),
        }
    }

    // -------------------------------------------------------------------------
    // RPC subscriptions
    // -------------------------------------------------------------------------

    pub async fn subscribe_new_heads(&self) -> anyhow::Result<Subscription<ExternalBlock>> {
        tracing::debug!("subscribing to newHeads event");

        let ws = self.require_ws()?;
        let result = ws
            .subscribe::<ExternalBlock, Vec<JsonValue>>("eth_subscribe", vec![JsonValue::String("newHeads".to_owned())], "eth_unsubscribe")
            .await;

        match result {
            Ok(sub) => Ok(sub),
            Err(e) => log_and_err!(reason = e, "failed to subscribe to newHeads event"),
        }
    }
}
