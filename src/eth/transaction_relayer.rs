use std::sync::Arc;

use anyhow::anyhow;
use ethers_core::types::Transaction;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use crate::eth::evm::EvmExecutionResult;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionInput;
use crate::eth::storage::StratusStorage;
use crate::infra::BlockchainClient;

pub struct TransactionRelayer {
    storage: Arc<StratusStorage>,

    /// RPC client that will submit transactions.
    chain: BlockchainClient,
}

impl TransactionRelayer {
    /// Creates a new [`TransactionRelayer`].
    pub fn new(storage: Arc<StratusStorage>, chain: BlockchainClient) -> Self {
        tracing::info!(?chain, "starting transaction relayer");
        Self { storage, chain }
    }

    /// Forwards the transaction to the external blockchain if the execution was successful on our side.
    #[tracing::instrument(skip_all)]
    pub async fn forward(&self, tx_input: TransactionInput, evm_result: EvmExecutionResult) -> anyhow::Result<()> {
        tracing::debug!(hash = %tx_input.hash, "forwarding transaction");

        // handle local failure
        if evm_result.is_failure() {
            tracing::debug!("transaction failed in local execution");
            let tx_execution = TransactionExecution::new_local(tx_input, evm_result);
            self.storage.save_execution(tx_execution).await?;
            return Ok(());
        }

        // handle local success
        let pending_tx = self.chain.send_raw_transaction(Transaction::from(tx_input.clone()).rlp()).await?;

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
                    "transaction_input": tx_input,
                    "stratus_execution": evm_result,
                    "substrate_receipt": receipt
                }
            );
            file.write_all(json.to_string().as_bytes()).await?;
            return Err(anyhow!("transaction succeeded in stratus but failed in substrate"));
        }

        Ok(())
    }
}
