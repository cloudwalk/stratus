use std::time::Duration;

use anyhow::Context;
use ethers_core::types::Bytes;
use ethers_core::types::Transaction;
use jsonrpsee::core::client::ClientT;
use jsonrpsee::core::client::Subscription;
use jsonrpsee::core::client::SubscriptionClientT;
use jsonrpsee::core::ClientError;
use jsonrpsee::http_client::HttpClient;
use jsonrpsee::http_client::HttpClientBuilder;
use jsonrpsee::ws_client::WsClient;
use jsonrpsee::ws_client::WsClientBuilder;
use serde_json::Value as JsonValue;
use tokio::sync::RwLock;
use tokio::sync::RwLockReadGuard;

use super::pending_transaction::PendingTransaction;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotValue;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::Wei;
use crate::ext::to_json_value;
use crate::ext::DisplayExt;
use crate::infra::tracing::TracingExt;
use crate::log_and_err;

#[derive(Debug)]
pub struct BlockchainClient {
    http: HttpClient,
    ws: Option<RwLock<WsClient>>,
    ws_url: Option<String>,
    timeout: Duration,
}

impl BlockchainClient {
    /// Creates a new RPC client connected only to HTTP.
    pub async fn new_http(http_url: &str, timeout: Duration) -> anyhow::Result<Self> {
        Self::new_http_ws(http_url, None, timeout).await
    }

    /// Creates a new RPC client connected to HTTP and optionally to WS.
    pub async fn new_http_ws(http_url: &str, ws_url: Option<&str>, timeout: Duration) -> anyhow::Result<Self> {
        tracing::info!(%http_url, "creating blockchain client");

        // build http provider
        let http = Self::build_http_client(http_url, timeout)?;

        // build ws provider
        let ws = if let Some(ws_url) = ws_url {
            Some(RwLock::new(Self::build_ws_client(ws_url, timeout).await?))
        } else {
            None
        };

        let client = Self {
            http,
            ws,
            ws_url: ws_url.map(|x| x.to_owned()),
            timeout,
        };

        // check health before assuming it is ok
        client.fetch_listening().await?;

        Ok(client)
    }

    fn build_http_client(url: &str, timeout: Duration) -> anyhow::Result<HttpClient> {
        tracing::info!(%url, timeout = %timeout.to_string_ext(), "creating blockchain http client");
        match HttpClientBuilder::default().request_timeout(timeout).build(url) {
            Ok(http) => {
                tracing::info!(%url, timeout = %timeout.to_string_ext(), "created blockchain http client");
                Ok(http)
            }
            Err(e) => {
                tracing::error!(reason = ?e, %url, timeout = %timeout.to_string_ext(), "failed to create blockchain http client");
                Err(e).context("failed to create blockchain http client")
            }
        }
    }

    async fn build_ws_client(url: &str, timeout: Duration) -> anyhow::Result<WsClient> {
        tracing::info!(%url, timeout = %timeout.to_string_ext(), "creating blockchain websocket client");
        match WsClientBuilder::new().connection_timeout(timeout).build(url).await {
            Ok(ws) => {
                tracing::info!(%url, timeout = %timeout.to_string_ext(), "created blockchain websocket client");
                Ok(ws)
            }
            Err(e) => {
                tracing::error!(reason = ?e, %url, timeout = %timeout.to_string_ext(), "failed to create blockchain websocket client");
                Err(e).context("failed to create blockchain websocket client")
            }
        }
    }

    // -------------------------------------------------------------------------
    // Websocket
    // -------------------------------------------------------------------------

    /// Checks if the supports websocket connection.
    pub fn supports_ws(&self) -> bool {
        self.ws.is_some()
    }

    /// Validates it is connected to websocket and returns a reference to the websocket client.
    async fn require_ws(&self) -> anyhow::Result<RwLockReadGuard<'_, WsClient>> {
        match &self.ws {
            Some(ws) => Ok(ws.read().await),
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

    /// Fetches the current block number.
    pub async fn fetch_block_number(&self) -> anyhow::Result<BlockNumber> {
        tracing::debug!("fetching block number");

        let result = self.http.request::<BlockNumber, Vec<()>>("eth_blockNumber", vec![]).await;

        match result {
            Ok(number) => Ok(number),
            Err(e) => log_and_err!(reason = e, "failed to fetch current block number"),
        }
    }

    /// Fetches a block by number.
    pub async fn fetch_block(&self, block_number: BlockNumber) -> anyhow::Result<JsonValue> {
        tracing::debug!(%block_number, "fetching block");

        let number = to_json_value(block_number);
        let result = self
            .http
            .request::<JsonValue, Vec<JsonValue>>("eth_getBlockByNumber", vec![number, JsonValue::Bool(true)])
            .await;

        match result {
            Ok(block) => Ok(block),
            Err(e) => log_and_err!(reason = e, "failed to fetch block by number"),
        }
    }

    /// Fetches a block by hash.
    pub async fn fetch_block_by_hash(&self, tx_hash: Hash, tx_detail: bool) -> anyhow::Result<JsonValue> {
        tracing::debug!(%tx_hash, "fetching block");

        let hash = to_json_value(tx_hash);
        let result = self
            .http
            .request::<JsonValue, Vec<JsonValue>>("eth_getBlockByHash", vec![hash, JsonValue::Bool(tx_detail)])
            .await;

        match result {
            Ok(block) => Ok(block),
            Err(e) => log_and_err!(reason = e, "failed to fetch block by hash"),
        }
    }

    /// Fetches a transaction by hash.
    pub async fn fetch_transaction(&self, tx_hash: Hash) -> anyhow::Result<Option<Transaction>> {
        tracing::debug!(%tx_hash, "fetching transaction");

        let hash = to_json_value(tx_hash);

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
    pub async fn fetch_receipt(&self, tx_hash: Hash) -> anyhow::Result<Option<ExternalReceipt>> {
        tracing::debug!(%tx_hash, "fetching transaction receipt");

        let hash = to_json_value(tx_hash);
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
    pub async fn fetch_balance(&self, address: &Address, block_number: Option<BlockNumber>) -> anyhow::Result<Wei> {
        tracing::debug!(%address, block_number = %block_number.or_empty(), "fetching account balance");

        let address = to_json_value(address);
        let number = to_json_value(block_number);
        let result = self.http.request::<Wei, Vec<JsonValue>>("eth_getBalance", vec![address, number]).await;

        match result {
            Ok(receipt) => Ok(receipt),
            Err(e) => log_and_err!(reason = e, "failed to fetch account balance"),
        }
    }

    /// Fetches a slot by a slot at some block.
    pub async fn fetch_storage_at(&self, address: &Address, index: &SlotIndex, point_in_time: StoragePointInTime) -> anyhow::Result<SlotValue> {
        tracing::debug!(%address, %point_in_time, "fetching account balance");

        let address = to_json_value(address);
        let index = to_json_value(index);
        let number = match point_in_time {
            StoragePointInTime::Present => to_json_value("latest"),
            StoragePointInTime::Past(number) => to_json_value(number),
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

    /// Fetches the current transaction count (nonce) for an account.
    pub async fn fetch_transaction_count(&self, address: &Address) -> anyhow::Result<Nonce> {
        tracing::debug!("fetching block number");
        let address = to_json_value(address);

        let result = self
            .http
            .request::<Nonce, Vec<JsonValue>>("eth_getTransactionCount", vec![address, to_json_value("latest")])
            .await;

        match result {
            Ok(number) => Ok(number),
            Err(e) => log_and_err!(reason = e, "failed to fetch transaction count"),
        }
    }

    // -------------------------------------------------------------------------
    // RPC mutations
    // -------------------------------------------------------------------------

    /// Sends a signed transaction.
    pub async fn send_raw_transaction(&self, tx: Bytes) -> anyhow::Result<PendingTransaction<'_>> {
        tracing::debug!("sending raw transaction");

        let tx = to_json_value(tx);
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

        let mut first_attempt = true;
        loop {
            let ws_read = self.require_ws().await?;
            let result = ws_read
                .subscribe::<ExternalBlock, Vec<JsonValue>>("eth_subscribe", vec![JsonValue::String("newHeads".to_owned())], "eth_unsubscribe")
                .await;

            match result {
                // subscribed
                Ok(sub) => return Ok(sub),

                // failed and need to reconnect
                e @ Err(ClientError::RestartNeeded(_)) => {
                    // will try to reconnect websocket client only in first attempt
                    if first_attempt {
                        tracing::error!(reason = ?e, %first_attempt, "failed to subscribe to newHeads event. trying to reconnect websocket client now.");
                    } else {
                        tracing::error!(reason = ?e, %first_attempt, "failed to subscribe to newHeads event. will not try to reconnect websocket client.");
                        return e.context("failed to subscribe to newHeads event");
                    }
                    first_attempt = false;

                    // reconnect websocket client
                    let new_ws_client = Self::build_ws_client(self.ws_url.as_ref().unwrap(), self.timeout).await?;
                    drop(ws_read);
                    let mut ws_write = self.ws.as_ref().unwrap().write().await;
                    let _ = std::mem::replace(&mut *ws_write, new_ws_client);
                }

                // failed and cannot do anything
                Err(e) => return log_and_err!(reason = e, "failed to subscribe to newHeads event"),
            }
        }
    }
}
