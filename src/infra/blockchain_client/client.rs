use std::time::Duration;

use ethers_core::types::Bytes;
use ethers_core::types::Transaction;
use jsonrpsee::core::client::ClientT;
use jsonrpsee::http_client::HttpClient;
use jsonrpsee::http_client::HttpClientBuilder;
use serde_json::Value as JsonValue;

use super::pending_transaction::PendingTransaction;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::Hash;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotValue;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::Wei;
use crate::log_and_err;

/// Default timeout for blockchain operations.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub struct BlockchainClient {
    http: HttpClient,
    pub url: String,
}

impl BlockchainClient {
    /// Creates a new RPC client.
    pub async fn new(url: &str) -> anyhow::Result<Self> {
        tracing::info!(%url, "starting blockchain client");

        // build provider
        let http = match HttpClientBuilder::default().request_timeout(DEFAULT_TIMEOUT).build(url) {
            Ok(http) => http,
            Err(e) => return log_and_err!(reason = e, "failed to create blockchain http client"),
        };
        let client = Self { http, url: url.to_string() };

        // check health before assuming it is ok
        client.check_health().await?;

        Ok(client)
    }

    pub async fn send_raw_transaction(&self, tx: Bytes) -> anyhow::Result<PendingTransaction<'_>> {
        let tx = serde_json::to_value(tx)?;
        let hash = self.http.request::<Hash, Vec<JsonValue>>("eth_sendRawTransaction", vec![tx]).await?;
        Ok(PendingTransaction::new(hash, self))
    }

    pub async fn get_transaction(&self, hash: Hash) -> anyhow::Result<Option<Transaction>> {
        let hash = serde_json::to_value(hash)?;
        Ok(self
            .http
            .request::<Option<Transaction>, Vec<JsonValue>>("eth_getTransactionByHash", vec![hash])
            .await?)
    }

    /// Checks if the blockchain is listening.
    pub async fn check_health(&self) -> anyhow::Result<()> {
        tracing::debug!("checking blockchain health");
        match self.http.request::<bool, Vec<()>>("net_listening", vec![]).await {
            Ok(_) => Ok(()),
            Err(e) => log_and_err!(reason = e, "failed to check blockchain health"),
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
    pub async fn get_transaction_receipt(&self, hash: Hash) -> anyhow::Result<Option<ExternalReceipt>> {
        tracing::debug!(%hash, "retrieving transaction receipt");

        let hash = serde_json::to_value(hash)?;
        let result = self
            .http
            .request::<Option<ExternalReceipt>, Vec<JsonValue>>("eth_getTransactionReceipt", vec![hash])
            .await;

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

    /// Retrieves a slot at some block.
    pub async fn get_storage_at(&self, address: &Address, index: &SlotIndex, point_in_time: StoragePointInTime) -> anyhow::Result<SlotValue> {
        tracing::debug!(%address, ?point_in_time, "retrieving account balance");

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
            Err(e) => log_and_err!(reason = e, "failed to retrieve account balance"),
        }
    }
}
