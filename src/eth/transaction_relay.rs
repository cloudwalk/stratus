use anyhow::anyhow;
use ethers_core::types::Transaction;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use crate::eth::primitives::Execution;
use crate::eth::primitives::ExecutionResult;
use crate::eth::primitives::TransactionInput;
use crate::infra::BlockchainClient;

pub struct TransactionRelay {
    // Provider for sending rpc calls to substrate
    provider: BlockchainClient,

    // Sender for transactions that failed on our side, and should be included in the next block
    pub failed_transactions: Mutex<Vec<(TransactionInput, Execution)>>,
}

impl TransactionRelay {
    /// Creates a new relay for forwarding transactions to another blockchain.
    pub async fn new(rpc_url: &str) -> anyhow::Result<Self> {
        Ok(Self {
            failed_transactions: Mutex::new(vec![]),
            provider: BlockchainClient::new(rpc_url).await?,
        })
    }

    /// Forwards the transaction to the external blockchain if the execution was successful on our side.
    pub async fn forward_transaction(&self, execution: Execution, transaction: TransactionInput) -> anyhow::Result<()> {
        tracing::debug!(?transaction.hash, "forwarding transaction");
        if execution.result == ExecutionResult::Success {
            let pending_tx = self.provider.send_raw_transaction(Transaction::from(transaction.clone()).rlp()).await?;

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
}
