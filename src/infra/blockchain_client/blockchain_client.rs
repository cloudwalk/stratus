#[cfg(feature = "replication")]
use std::str::FromStr;
use std::time::Duration;

#[cfg(feature = "replication")]
use alloy_primitives::hex;
use anyhow::Context;
use jsonrpsee::core::ClientError;
use jsonrpsee::core::client::ClientT;
use jsonrpsee::core::client::Subscription;
use jsonrpsee::core::client::SubscriptionClientT;
use jsonrpsee::http_client::HttpClient;
use jsonrpsee::http_client::HttpClientBuilder;
#[cfg(feature = "replication")]
use jsonrpsee::types::error::METHOD_NOT_FOUND_CODE;
use jsonrpsee::ws_client::WsClient;
use jsonrpsee::ws_client::WsClientBuilder;
use tokio::sync::RwLock;
use tokio::sync::RwLockReadGuard;

use crate::GlobalState;
use crate::alias::AlloyBytes;
use crate::alias::AlloyTransaction;
use crate::alias::JsonValue;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
#[cfg(feature = "replication")]
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalBlockWithReceipts;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::Hash;
use crate::eth::primitives::StratusError;
use crate::eth::primitives::TransactionError;
use crate::eth::primitives::Wei;
use crate::eth::rpc::RpcClientApp;
#[cfg(feature = "replication")]
use crate::eth::storage::permanent::rocks::types::ReplicationLogRocksdb;
use crate::ext::DisplayExt;
use crate::ext::to_json_value;
use crate::infra::tracing::TracingExt;
use crate::log_and_err;

#[derive(Debug)]
pub struct BlockchainClient {
    http: HttpClient,
    pub http_url: String,
    ws: Option<RwLock<WsClient>>,
    ws_url: Option<String>,
    timeout: Duration,
    #[allow(dead_code)]
    max_response_size_bytes: u32,
}

impl BlockchainClient {
    /// Creates a new RPC client connected only to HTTP.
    pub async fn new_http(http_url: &str, timeout: Duration, max_response_size_bytes: u32) -> anyhow::Result<Self> {
        Self::new_http_ws(http_url, None, timeout, max_response_size_bytes).await
    }

    /// Creates a new RPC client connected to HTTP and optionally to WS.
    pub async fn new_http_ws(http_url: &str, ws_url: Option<&str>, timeout: Duration, max_response_size_bytes: u32) -> anyhow::Result<Self> {
        tracing::info!(%http_url, "creating blockchain client");

        // build http provider
        let http = Self::build_http_client(http_url, timeout, max_response_size_bytes)?;

        // build ws provider
        let ws = if let Some(ws_url) = ws_url {
            Some(RwLock::new(Self::build_ws_client(ws_url, timeout).await?))
        } else {
            None
        };

        let client = Self {
            http,
            http_url: http_url.to_owned(),
            ws,
            ws_url: ws_url.map(|x| x.to_owned()),
            timeout,
            max_response_size_bytes,
        };

        // check health before assuming it is ok
        client.fetch_listening().await?;

        Ok(client)
    }

    fn build_http_client(url: &str, timeout: Duration, max_response_size_bytes: u32) -> anyhow::Result<HttpClient> {
        tracing::info!(%url, timeout = %timeout.to_string_ext(), "creating blockchain http client");
        match HttpClientBuilder::default()
            .request_timeout(timeout)
            .max_response_size(max_response_size_bytes)
            .build(url)
        {
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

        let result = self.http.request::<bool, _>("net_listening", [(); 0]).await;
        match result {
            Ok(_) => Ok(()),
            Err(e) => log_and_err!(reason = e, "failed to fetch listening status"),
        }
    }

    /// Fetches the current block number.
    pub async fn fetch_block_number(&self) -> anyhow::Result<BlockNumber> {
        tracing::debug!("fetching block number");

        let result = self.http.request::<BlockNumber, _>("eth_blockNumber", [(); 0]).await;

        match result {
            Ok(number) => Ok(number),
            Err(e) => log_and_err!(reason = e, "failed to fetch current block number"),
        }
    }

    /// Fetches a block by number with receipts.
    pub async fn fetch_block_and_receipts(&self, block_number: BlockNumber) -> anyhow::Result<Option<ExternalBlockWithReceipts>> {
        tracing::debug!(%block_number, "fetching block");

        let number = to_json_value(block_number);
        let result = self
            .http
            .request::<Option<ExternalBlockWithReceipts>, _>("stratus_getBlockAndReceipts", [number])
            .await;

        match result {
            Ok(block) => Ok(block),
            Err(e) => log_and_err!(reason = e, "failed to fetch block with receipts"),
        }
    }

    /// Fetches a block by number.
    pub async fn fetch_block(&self, block_number: BlockNumber) -> anyhow::Result<Option<ExternalBlock>> {
        tracing::debug!(%block_number, "fetching block");

        let number = to_json_value(block_number);
        let result = self
            .http
            .request::<Option<ExternalBlock>, _>("eth_getBlockByNumber", [number, JsonValue::Bool(true)])
            .await;

        match result {
            Ok(block) => Ok(block),
            Err(e) => log_and_err!(reason = e, "failed to fetch block by number"),
        }
    }

    /// Fetches a transaction by hash.
    pub async fn fetch_transaction(&self, tx_hash: Hash) -> anyhow::Result<Option<AlloyTransaction>> {
        tracing::debug!(%tx_hash, "fetching transaction");

        let hash = to_json_value(tx_hash);
        let result = self.http.request::<Option<AlloyTransaction>, _>("eth_getTransactionByHash", [hash]).await;

        match result {
            Ok(tx) => Ok(tx),
            Err(e) => log_and_err!(reason = e, "failed to fetch transaction by hash"),
        }
    }

    /// Fetches a receipt by hash.
    pub async fn fetch_receipt(&self, tx_hash: Hash) -> anyhow::Result<Option<ExternalReceipt>> {
        tracing::debug!(%tx_hash, "fetching transaction receipt");

        let hash = to_json_value(tx_hash);
        let result = self.http.request::<Option<ExternalReceipt>, _>("eth_getTransactionReceipt", [hash]).await;

        match result {
            Ok(receipt) => Ok(receipt),
            Err(e) => log_and_err!(reason = e, "failed to fetch transaction receipt by hash"),
        }
    }

    /// Fetches account balance by address and block number.
    pub async fn fetch_balance(&self, address: Address, block_number: Option<BlockNumber>) -> anyhow::Result<Wei> {
        tracing::debug!(%address, block_number = %block_number.or_empty(), "fetching account balance");

        let address = to_json_value(address);
        let number = to_json_value(block_number);
        let result = self.http.request::<Wei, _>("eth_getBalance", [address, number]).await;

        match result {
            Ok(receipt) => Ok(receipt),
            Err(e) => log_and_err!(reason = e, "failed to fetch account balance"),
        }
    }

    /// Returns the external replication log if found.
    #[cfg(feature = "replication")]
    pub async fn fetch_replication_log(&self, block_number: BlockNumber) -> anyhow::Result<Option<ReplicationLogRocksdb>> {
        tracing::debug!(%block_number, "fetching replication log");

        let number = to_json_value(block_number);
        let result = self.http.request::<Option<serde_json::Value>, _>("stratus_getReplicationLog", [number]).await;

        match result {
            Ok(Some(json_value)) => {
                let block_number_str = json_value["block_number"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("invalid block_number in response"))?;

                let block_number = BlockNumber::from_str(block_number_str).map_err(|_| anyhow::anyhow!("invalid block_number format in response"))?;

                let hex_string = json_value["replication_log"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("invalid replication_log in response"))?;

                match hex::decode(hex_string) {
                    Ok(decoded) => {
                        let log_data = Bytes(decoded);
                        tracing::debug!(block_number = %block_number, decoded_size = log_data.len(), "decoded replication log");
                        Ok(Some(ReplicationLogRocksdb::new(block_number, log_data)))
                    }
                    Err(e) => log_and_err!(reason = e, "failed to decode replication log hex"),
                }
            }
            Ok(None) => Ok(None),
            Err(ClientError::Call(err)) if err.code() == METHOD_NOT_FOUND_CODE => {
                let message = GlobalState::shutdown_from(
                    "Importer (RocksDB Replication)",
                    "stratus_getReplicationLog is required for RocksDB replication",
                );
                log_and_err!(reason = err, message)
            }
            Err(e) => log_and_err!(reason = e, "failed to fetch replication log"),
        }
    }

    // -------------------------------------------------------------------------
    // RPC mutations
    // -------------------------------------------------------------------------

    /// Forwards a transaction to leader.
    pub async fn send_raw_transaction_to_leader(&self, tx: AlloyBytes, rpc_client: &RpcClientApp) -> Result<Hash, StratusError> {
        tracing::debug!("sending raw transaction to leader");

        let tx = to_json_value(tx);
        let rpc_client = to_json_value(rpc_client);
        let result = self.http.request::<Hash, _>("eth_sendRawTransaction", [tx, rpc_client]).await;

        match result {
            Ok(hash) => Ok(hash),
            Err(ClientError::Call(response)) => Err(TransactionError::LeaderFailed(response.into_owned()).into()),
            Err(e) => {
                tracing::error!(reason = ?e, "failed to send raw transaction to leader");
                Err(TransactionError::ForwardToLeaderFailed.into())
            }
        }
    }

    // -------------------------------------------------------------------------
    // RPC subscriptions
    // -------------------------------------------------------------------------

    pub async fn subscribe_new_heads(&self) -> anyhow::Result<Subscription<ExternalBlock>> {
        const TASK_NAME: &str = "blockchain::subscribe_new_heads";
        tracing::debug!("subscribing to newHeads event");

        let mut first_attempt = true;
        loop {
            if GlobalState::is_shutdown_warn(TASK_NAME) {
                return Err(anyhow::anyhow!("shutdown warning"));
            };

            let ws_read = self.require_ws().await?;
            let result = ws_read
                .subscribe::<ExternalBlock, _>("eth_subscribe", [JsonValue::String("newHeads".to_owned())], "eth_unsubscribe")
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
