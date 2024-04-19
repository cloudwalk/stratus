use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

use ethers_core::types::Bytes;
use ethers_core::types::Transaction;
use futures::Future;
use futures::Stream;
use futures_timer::Delay;
use futures_util::{stream, FutureExt, StreamExt};
use jsonrpsee::core::client::ClientT;
use jsonrpsee::http_client::HttpClient;
use jsonrpsee::http_client::HttpClientBuilder;
use pin_project::pin_project;
use serde_json::Value as JsonValue;

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

pub struct BlockchainClient {
    http: HttpClient,
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
        let client = Self { http };

        // check health before assuming it is ok
        client.check_health().await?;

        Ok(client)
    }

    pub async fn send_raw_transaction<'a>(&'a self, tx: Bytes) -> anyhow::Result<PendingTransaction<'a>> {
        let tx = serde_json::to_value(tx)?;
        let hash = self.http.request::<Hash, Vec<JsonValue>>("eth_sendRawTransaction", vec![tx]).await?;
        Ok(PendingTransaction::new(hash, self))
    }

    pub async fn get_transaction(&self, hash: Hash) -> anyhow::Result<Option<Transaction>> {
        let hash = serde_json::to_value(hash)?;
        Ok(self
            .http
            .request::<Option<Transaction>, Vec<JsonValue>>("eth_getTransaction", vec![hash])
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

type PinBoxFut<'a, T> = Pin<Box<dyn Future<Output = anyhow::Result<T>> + Send + 'a>>;

/// Create a stream that emits items at a fixed interval. Used for rate control
pub fn interval(duration: Duration) -> impl futures::stream::Stream<Item = ()> + Send + Unpin {
    stream::unfold((), move |_| Delay::new(duration).map(|_| Some(((), ())))).map(drop)
}

enum PendingTxState<'a> {
    /// Initial delay to ensure the GettingTx loop doesn't immediately fail
    InitialDelay(Pin<Box<Delay>>),

    /// Waiting for interval to elapse before calling API again
    PausedGettingTx,

    /// Polling The blockchain to see if the Tx has confirmed or dropped
    GettingTx(PinBoxFut<'a, Option<Transaction>>),

    /// Waiting for interval to elapse before calling API again
    PausedGettingReceipt,

    /// Polling the blockchain for the receipt
    GettingReceipt(PinBoxFut<'a, Option<ExternalReceipt>>),

    CheckingReceipt(Option<ExternalReceipt>),

    /// Future has completed and should panic if polled again
    Completed,
}

#[pin_project]
pub struct PendingTransaction<'a> {
    state: PendingTxState<'a>,
    provider: &'a BlockchainClient,
    tx_hash: Hash,
    interval: Box<dyn Stream<Item = ()> + Send + Unpin>,
    retries_remaining: i32,
}

impl<'a> PendingTransaction<'a> {
    pub fn new(tx_hash: Hash, provider: &'a BlockchainClient) -> Self {
        let delay = Box::pin(Delay::new(Duration::from_secs(1)));
        PendingTransaction {
            state: PendingTxState::InitialDelay(delay),
            provider,
            tx_hash,
            interval: Box::new(interval(Duration::from_secs(1))),
            retries_remaining: 3,
        }
    }
}

impl<'a> Future for PendingTransaction<'a> {
    type Output = anyhow::Result<Option<ExternalReceipt>>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        let this = self.project();

        match this.state {
            PendingTxState::InitialDelay(fut) => {
                futures_util::ready!(fut.as_mut().poll(ctx));
                let fut = Box::pin(this.provider.get_transaction((*this.tx_hash).clone()));
                *this.state = PendingTxState::GettingTx(fut);
                ctx.waker().wake_by_ref();
                return Poll::Pending;
            }
            PendingTxState::PausedGettingTx => {
                // Wait the polling period so that we do not spam the chain when no
                // new block has been mined
                let _ready = futures_util::ready!(this.interval.poll_next_unpin(ctx));
                let fut = Box::pin(this.provider.get_transaction((*this.tx_hash).clone()));
                *this.state = PendingTxState::GettingTx(fut);
                ctx.waker().wake_by_ref();
            }
            PendingTxState::GettingTx(fut) => {
                let tx_res = futures_util::ready!(fut.as_mut().poll(ctx));
                // If the provider errors, just try again after the interval.
                // nbd.
                if tx_res.is_err() {
                    *this.state = PendingTxState::PausedGettingTx;
                    ctx.waker().wake_by_ref();
                    return Poll::Pending;
                }

                let tx_opt = tx_res.unwrap();

                if tx_opt.is_none() {
                    if *this.retries_remaining == 0 {
                        tracing::debug!("Dropped from mempool, pending tx {:?}", *this.tx_hash);
                        *this.state = PendingTxState::Completed;
                        return Poll::Ready(Ok(None));
                    }

                    *this.retries_remaining -= 1;
                    *this.state = PendingTxState::PausedGettingTx;
                    ctx.waker().wake_by_ref();
                    return Poll::Pending;
                }

                // If it hasn't confirmed yet, poll again later
                let tx = tx_opt.unwrap();
                if tx.block_number.is_none() {
                    *this.state = PendingTxState::PausedGettingTx;
                    ctx.waker().wake_by_ref();
                    return Poll::Pending;
                }

                // Start polling for the receipt now
                let fut = Box::pin(this.provider.get_transaction_receipt((*this.tx_hash).clone()));
                *this.state = PendingTxState::GettingReceipt(fut);
                ctx.waker().wake_by_ref();
                return Poll::Pending;
            }
            PendingTxState::PausedGettingReceipt => {
                // Wait the polling period so that we do not spam the chain when no
                // new block has been mined
                let _ready = futures_util::ready!(this.interval.poll_next_unpin(ctx));
                let fut = Box::pin(this.provider.get_transaction_receipt((*this.tx_hash).clone()));
                *this.state = PendingTxState::GettingReceipt(fut);
                ctx.waker().wake_by_ref();
            }
            PendingTxState::GettingReceipt(fut) => {
                if let Ok(receipt) = futures_util::ready!(fut.as_mut().poll(ctx)) {
                    *this.state = PendingTxState::CheckingReceipt(receipt)
                } else {
                    *this.state = PendingTxState::PausedGettingReceipt
                }
                ctx.waker().wake_by_ref();
            }
            PendingTxState::CheckingReceipt(receipt) => {
                if receipt.is_none() {
                    *this.state = PendingTxState::PausedGettingReceipt;
                    ctx.waker().wake_by_ref();
                    return Poll::Pending;
                }

                let receipt = receipt.take();
                *this.state = PendingTxState::Completed;
                return Poll::Ready(Ok(receipt));
            }
            PendingTxState::Completed => {
                panic!("polled pending transaction future after completion")
            }
        };

        Poll::Pending
    }
}
