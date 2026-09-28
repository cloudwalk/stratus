use std::sync::Arc;

use alloy_rpc_types_eth::BlockTransactions;
use anyhow::anyhow;
use anyhow::bail;

use crate::eth::follower::importer::BlockchainClient;
use crate::eth::follower::importer::fetch_with_retry;
use crate::eth::follower::importer::fetchers::DataFetcher;
use crate::eth::rpc::pagination::ResponseFormat;
use crate::eth::types::Block;
use crate::eth::types::BlockNumber;
use crate::eth::types::ExternalBlock;
use crate::eth::types::ExternalReceipt;

/// Block with receipts fetched from the leader, in either supported response format.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum FetchedBlockWithReceipts {
    /// Legacy alloy format: block and receipts as separate alloy RPC types.
    Alloy { block: ExternalBlock, receipts: Vec<ExternalReceipt> },

    /// Stratus-native format: block with receipts embedded, deserialized from the storage DTO.
    Stratus(Block),
}

impl FetchedBlockWithReceipts {
    /// Returns the block number, regardless of the format it was fetched in.
    pub fn block_number(&self) -> BlockNumber {
        match self {
            Self::Alloy { block, .. } => block.number(),
            Self::Stratus(block) => block.number(),
        }
    }

    /// Returns the number of transactions (and therefore of receipts), regardless of the format.
    pub fn receipts_len(&self) -> usize {
        match self {
            Self::Alloy { receipts, .. } => receipts.len(),
            Self::Stratus(block) => block.transactions.len(),
        }
    }
}

pub struct BlockWithReceiptsFetcher {
    pub chain: Arc<BlockchainClient>,
    pub response_format: ResponseFormat,
}

impl DataFetcher for BlockWithReceiptsFetcher {
    type FetchedType = FetchedBlockWithReceipts;
    type PostProcessType = FetchedBlockWithReceipts;

    async fn fetch(&self, block_number: BlockNumber) -> Self::FetchedType {
        let fetch_fn = |bn| {
            let chain = Arc::clone(&self.chain);
            let response_format = self.response_format;
            async move { chain.fetch_block_and_receipts(bn, response_format).await }
        };

        fetch_with_retry(block_number, fetch_fn, "block and receipts").await
    }

    async fn post_process(&self, data: Self::FetchedType) -> anyhow::Result<Self::PostProcessType> {
        match data {
            FetchedBlockWithReceipts::Alloy { mut block, mut receipts } => {
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
                transactions.sort_by_key(|a| a.transaction_index);
                receipts.sort_by_key(|a| a.transaction_index);

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

                Ok(FetchedBlockWithReceipts::Alloy { block, receipts })
            }
            FetchedBlockWithReceipts::Stratus(block) => {
                for window in block.transactions.windows(2) {
                    let tx_index = window[0].mined_data.index.0;
                    let next_tx_index = window[1].mined_data.index.0;
                    if tx_index + 1 != next_tx_index {
                        tracing::error!(tx_index, next_tx_index, "two consecutive transactions must have consecutive indices");
                    }
                }

                Ok(FetchedBlockWithReceipts::Stratus(block))
            }
        }
    }
}
