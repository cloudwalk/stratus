use std::fmt::Debug;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

use ethers_core::types::Transaction;
use futures::Future;
use futures::Stream;
use futures_timer::Delay;
use futures_util::stream;
use futures_util::FutureExt;
use futures_util::StreamExt;
use pin_project::pin_project;

use super::BlockchainClient;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::Hash;

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

impl<'a> Debug for PendingTxState<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InitialDelay(_) => "InitialDelay",
            Self::CheckingReceipt(_) => "CheckingReceipt",
            Self::Completed => "Completed",
            Self::GettingReceipt(_) => "GettingReceipt",
            Self::GettingTx(_) => "GettingTx",
            Self::PausedGettingReceipt => "PausedGettingReceipt",
            Self::PausedGettingTx => "PausedGettingTx",
        }
        .fmt(f)
    }
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
        tracing::debug!(?this.state);

        match this.state {
            PendingTxState::InitialDelay(fut) => {
                futures_util::ready!(fut.as_mut().poll(ctx));
                let fut = Box::pin(this.provider.fetch_transaction(*this.tx_hash));
                *this.state = PendingTxState::GettingTx(fut);
                ctx.waker().wake_by_ref();
                return Poll::Pending;
            }
            PendingTxState::PausedGettingTx => {
                // Wait the polling period so that we do not spam the chain when no
                // new block has been mined
                let _ready = futures_util::ready!(this.interval.poll_next_unpin(ctx));
                let fut = Box::pin(this.provider.fetch_transaction(*this.tx_hash));
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
                let fut = Box::pin(this.provider.fetch_receipt(*this.tx_hash));
                *this.state = PendingTxState::GettingReceipt(fut);
                ctx.waker().wake_by_ref();
                return Poll::Pending;
            }
            PendingTxState::PausedGettingReceipt => {
                // Wait the polling period so that we do not spam the chain when no
                // new block has been mined
                let _ready = futures_util::ready!(this.interval.poll_next_unpin(ctx));
                let fut = Box::pin(this.provider.fetch_receipt(*this.tx_hash));
                *this.state = PendingTxState::GettingReceipt(fut);
                ctx.waker().wake_by_ref();
            }
            PendingTxState::GettingReceipt(fut) => {
                if let Ok(receipt) = futures_util::ready!(fut.as_mut().poll(ctx)) {
                    *this.state = PendingTxState::CheckingReceipt(receipt);
                } else {
                    *this.state = PendingTxState::PausedGettingReceipt;
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
