use std::sync::Arc;

use ethers_core::types::Transaction;

use crate::eth::primitives::TransactionInput;
use crate::infra::blockchain_client::pending_transaction::PendingTransaction;
use crate::infra::BlockchainClient;

pub struct TransactionRelayer {
    /// RPC client that will submit transactions.
    chain: Arc<BlockchainClient>,
}

impl TransactionRelayer {
    /// Creates a new [`TransactionRelayer`].
    pub fn new(chain: Arc<BlockchainClient>) -> Self {
        tracing::info!(?chain, "creating transaction relayer");
        Self { chain }
    }

    /// Forwards the transaction to the external blockchain if the execution was successful on our side.
    #[tracing::instrument(skip_all)]
    pub async fn forward(&self, tx_input: TransactionInput) -> anyhow::Result<PendingTransaction> {
        tracing::debug!(hash = %tx_input.hash, "forwarding transaction");

        let tx = self.chain.send_raw_transaction(Transaction::from(tx_input.clone()).rlp()).await?;

        Ok(tx)
    }
}
