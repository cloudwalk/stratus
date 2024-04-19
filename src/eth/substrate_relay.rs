use std::sync::Arc;

use anyhow::anyhow;
use ethers::providers::Http;
use ethers::providers::Middleware;
use ethers::providers::Provider;
use ethers_core::types::Transaction;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::broadcast;
use tokio::sync::oneshot;
use tokio::sync::Mutex;

use crate::eth::evm::EvmExecutionResult;
use crate::eth::evm::EvmInput;
use crate::eth::primitives::Block;
use crate::eth::primitives::CallInput;
use crate::eth::primitives::Execution;
use crate::eth::primitives::ExecutionMetrics;
use crate::eth::primitives::ExecutionResult;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalReceipts;
use crate::eth::primitives::ExternalTransactionExecution;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionInput;
use crate::eth::storage::StorageError;
use crate::eth::storage::StratusStorage;
use crate::eth::BlockMiner;
#[cfg(feature = "metrics")]
use crate::infra::metrics;

#[cfg(feature = "forward_transaction")]
pub struct SubstrateRelay {
    // Provider for sending rpc calls to substrate
    provider: Provider<Http>,

    // Sender for transactions that failed on our side, and should be included in the next block
    pub failed_transactions: Mutex<Vec<(TransactionInput, Execution)>>,
}

#[cfg(feature = "forward_transaction")]
impl SubstrateRelay {
    pub fn new(substrate_rpc_url: &str) -> Self {
        Self {
            failed_transactions: Mutex::new(vec![]),
            provider: Provider::<Http>::try_from(substrate_rpc_url).expect("could not instantiate HTTP Provider"),
        }
    }

    pub async fn forward_transaction(&self, execution: Execution, transaction: TransactionInput) -> anyhow::Result<()> {
        if execution.result == ExecutionResult::Success {
            let pending_tx = self.provider.send_raw_transaction(Transaction::from(transaction).rlp()).await?;

            let Some(receipt) = pending_tx.await? else {
                return Err(anyhow!("transaction did not produce a receipt"));
            };

            let status = match receipt.status {
                Some(status) => status.as_u32(),
                None => return Err(anyhow!("receipt did not report the transaction status")),
            };

            if status == 0 {
                let mut file = File::create(format!("data/mismatched_transactions/{}", receipt.transaction_hash.clone())).await?;
                file.write_all(serde_json::to_string(&receipt)?.as_bytes()).await?;
                return Err(anyhow!("transaction succeeded in stratus but failed in substrate"));
            }
        } else {
            self.failed_transactions.lock().await.push((transaction, execution));
        }

        Ok(())
    }
}
