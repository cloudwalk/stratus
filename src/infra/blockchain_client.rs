use anyhow::Context;
use hex_literal::hex;
use jsonrpsee::core::client::ClientT;
use jsonrpsee::http_client::HttpClient;
use jsonrpsee::http_client::HttpClientBuilder;
use serde_json::Value as JsonValue;

use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;

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

        let number = serde_json::to_value(number)?;
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

    pub async fn get_transaction_receipt(&self, transaction_hash: Hash) -> anyhow::Result<JsonValue> {
        tracing::debug!(%transaction_hash, "retrieving block");

        let serde_transaction_hash = serde_json::to_value(transaction_hash)?;
        match self
            .http
            .request::<JsonValue, Vec<JsonValue>>("eth_getTransactionReceipt", vec![serde_transaction_hash])
            .await
        {
            Ok(block) => Ok(block),
            Err(e) => {
                tracing::error!(reason = ?e, "failed to retrieve block by transactionHash");
                Err(e).context("failed to retrieve block by number")
            }
        }
    }

    pub async fn get_transaction_slots(&self, contract_hash: Address, number: BlockNumber) -> anyhow::Result<JsonValue> {

        let serde_transaction_hash = serde_json::to_value(contract_hash)?;
        let serde_slot = serde_json::to_value(Hash::new(hex!("0000000000000000000000000000000000000000000000000000000000000000")))?;
        let serde_number = serde_json::to_value(number)?;

        match self
            .http
            .request::<JsonValue, Vec<JsonValue>>("eth_getStorageAt", vec![serde_transaction_hash, serde_slot, serde_number])
            .await
        {
            Ok(block) => Ok(block),
            Err(e) => {
                tracing::error!(reason = ?e, "failed to retrieve block by transactionHash");
                Err(e).context("failed to retrieve block by number")
            }
        }
    }
}
