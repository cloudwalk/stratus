use std::sync::Arc;

use alloy_rpc_types_eth::BlockTransactions;
use anyhow::anyhow;
use anyhow::bail;
use async_trait::async_trait;

use crate::eth::follower::importer::fetch_with_retry;
use crate::eth::follower::importer::fetchers::FetcherWorker;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalReceipt;
use crate::infra::BlockchainClient;

pub struct ExecutionFetcherWorker {
    pub chain: Arc<BlockchainClient>,
}

#[async_trait]
impl FetcherWorker<(ExternalBlock, Vec<ExternalReceipt>), (ExternalBlock, Vec<ExternalReceipt>)> for ExecutionFetcherWorker {
    async fn fetch(&self, block_number: BlockNumber) -> (ExternalBlock, Vec<ExternalReceipt>) {
        let fetch_fn = |bn| {
            let chain = Arc::clone(&self.chain);
            async move {
                chain
                    .fetch_block_and_receipts(bn)
                    .await
                    .map(|opt| opt.map(|response| (response.block, response.receipts)))
            }
        };

        fetch_with_retry(block_number, fetch_fn, "block and receipts").await
    }

    async fn post_process(&self, data: (ExternalBlock, Vec<ExternalReceipt>)) -> anyhow::Result<(ExternalBlock, Vec<ExternalReceipt>)> {
        let (mut block, mut receipts) = data;
        let block_number = block.number();
        let BlockTransactions::Full(transactions) = &mut block.transactions else {
            bail!("expected full transactions, got hashes or uncle");
        };

        if transactions.len() != receipts.len() {
            bail!(
                "block {} has mismatched transaction and receipt length: {} transactions but {} receipts",
                block_number,
                transactions.len(),
                receipts.len()
            );
        }

        // Stably sort transactions and receipts by transaction_index
        transactions.sort_by(|a, b| a.transaction_index.cmp(&b.transaction_index));
        receipts.sort_by(|a, b| a.transaction_index.cmp(&b.transaction_index));

        // perform additional checks on the transaction index
        for window in transactions.windows(2) {
            let tx_index = window[0].transaction_index.ok_or(anyhow!("missing transaction index"))? as u32;
            let next_tx_index = window[1].transaction_index.ok_or(anyhow!("missing transaction index"))? as u32;
            if tx_index + 1 != next_tx_index {
                tracing::error!(tx_index, next_tx_index, "two consecutive transactions must have consecutive indices");
            }
        }
        for window in receipts.windows(2) {
            let tx_index = window[0].transaction_index.ok_or(anyhow!("missing transaction index"))? as u32;
            let next_tx_index = window[1].transaction_index.ok_or(anyhow!("missing transaction index"))? as u32;
            if tx_index + 1 != next_tx_index {
                tracing::error!(tx_index, next_tx_index, "two consecutive receipts must have consecutive indices");
            }
        }

        Ok((block, receipts))
    }
}
