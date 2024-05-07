use std::sync::Arc;

use anyhow::anyhow;
use ethers_core::types::Transaction;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::ExecutionResult;
use crate::eth::primitives::TransactionInput;
use crate::eth::storage::StratusStorage;
use crate::infra::BlockchainClient;

pub struct TransactionRelayer {
    /// TODO: implement storage use.
    _storage: Arc<StratusStorage>,

    /// RPC client that will submit transactions.
    chain: BlockchainClient,

    /// TODO: remove it because failed transactions will be kept in temporary storage.
    pub failed_transactions: Mutex<Vec<(TransactionInput, EvmExecution)>>,
}

impl TransactionRelayer {
    /// Creates a new [`TransactionRelayer`].
    pub fn new(storage: Arc<StratusStorage>, chain: BlockchainClient) -> Self {
        tracing::info!(?chain, "creating transaction relayer");
        Self {
            _storage: storage,
            chain,
            failed_transactions: Mutex::new(vec![]),
        }
    }

    /// Forwards the transaction to the external blockchain if the execution was successful on our side.
    #[tracing::instrument(skip_all)]
    pub async fn forward_transaction(&self, execution: EvmExecution, transaction: TransactionInput) -> anyhow::Result<()> {
        tracing::debug!(?transaction.hash, "forwarding transaction");
        if execution.result == ExecutionResult::Success {
            let pending_tx = self.chain.send_raw_transaction(Transaction::from(transaction.clone()).rlp()).await?;

            let Some(receipt) = pending_tx.await? else {
                return Err(anyhow!("transaction did not produce a receipt"));
            };

            let status = match receipt.status {
                Some(status) => status.as_u32(),
                None => return Err(anyhow!("receipt did not report the transaction status")),
            };

            if status == 0 {
                tracing::warn!(?receipt.transaction_hash, "transaction result mismatch between stratus and external rpc. saving to json");
                let mut file = File::create(format!("data/mismatched_transactions/{}.json", receipt.transaction_hash)).await?;
                let json = serde_json::json!(
                    {
                        "transaction_input": transaction,
                        "stratus_execution": execution,
                        "substrate_receipt": receipt
                    }
                );
                file.write_all(json.to_string().as_bytes()).await?;
                return Err(anyhow!("transaction succeeded in stratus but failed in substrate"));
            }
        } else {
            tracing::debug!("transaction failed in substrate, pushing to failed transactions");
            self.failed_transactions.lock().await.push((transaction, execution));
        }

        Ok(())
    }

    /// Drain failed transactions.
    pub async fn drain_failed_transactions(&self) -> Vec<(TransactionInput, EvmExecution)> {
        let mut failed_tx_lock = self.failed_transactions.lock().await;
        failed_tx_lock.drain(..).collect::<Vec<_>>()
    }
}
