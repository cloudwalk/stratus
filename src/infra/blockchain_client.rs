use anyhow::Context;
use jsonrpsee::core::client::ClientT;
use jsonrpsee::http_client::HttpClient;
use jsonrpsee::http_client::HttpClientBuilder;
use serde_json::Value as JsonValue;

use crate::eth::primitives::BlockNumber;

pub struct BlockchainClient {
    http: HttpClient,
}

impl BlockchainClient {
    /// Creates a new RPC client.
    pub fn new(url: &str) -> anyhow::Result<Self> {
        tracing::info!(%url, "initing blockchain client");
        let http = match HttpClientBuilder::default().build(url) {
            Ok(client) => client,
            Err(e) => {
                tracing::error!(reason = ?e, "failed to create blockchain http client");
                return Err(e).context("failed to create blockchain http client");
            }
        };
        Ok(Self { http })
    }

    /// Retrieves the current block number.
    pub async fn get_current_block_number(&self) -> anyhow::Result<BlockNumber> {
        tracing::debug!("retrieving current block number");
        match self.http.request::<BlockNumber, Vec<()>>("eth_blockNumber", vec![]).await {
            Ok(number) => Ok(number),
            Err(e) => {
                tracing::error!(reason = ?e, "failed to retrieve current block number");
                Err(e).context("failed to retrieve current block number")
            }
        }
    }

    /// Retrieves a block by number.
    pub async fn get_block_by_number(&self, number: BlockNumber) -> anyhow::Result<JsonValue> {
        tracing::debug!(%number, "retrieving block");

        let number = serde_json::to_value(number).unwrap();
        match self
            .http
            .request::<JsonValue, Vec<JsonValue>>("eth_getBlockByNumber", vec![number, JsonValue::Bool(true)])
            .await
        {
            Ok(block) => Ok(block),
            Err(e) => {
                tracing::error!(reason = ?e, "failed to retrieve block by number");
                Err(e).context("failed to retrieve block by number")
            }
        }
    }
}
