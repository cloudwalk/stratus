use std::sync::Arc;

use alloy_rpc_types_eth::BlockTransactions;
use anyhow::anyhow;
use anyhow::bail;

use crate::eth::executor::ExecutionResult;
use crate::eth::executor::TransactionExecution;
use crate::eth::executor::TransactionExecutionInput;
use crate::eth::executor::TransactionExecutionResult;
use crate::eth::follower::importer::BlockchainClient;
use crate::eth::follower::importer::fetch_with_retry;
use crate::eth::follower::importer::fetchers::DataFetcher;
use crate::eth::follower::importer::importers::blockchain_client::BlockAndReceiptsResponse;
use crate::eth::types::Address;
use crate::eth::types::Block;
use crate::eth::types::BlockInfo;
use crate::eth::types::BlockNumber;
use crate::eth::types::Bytes;
use crate::eth::types::ExternalBlock;
use crate::eth::types::ExternalReceipt;
use crate::eth::types::Gas;
use crate::eth::types::Index;
use crate::eth::types::Log;
use crate::eth::types::Size;
use crate::eth::types::TransactionInput;
use crate::eth::types::TransactionMined;

/// Origin of an imported block, decided by the wire format the leader answered in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockOrigin {
    /// Answered in the stratus-native format.
    Stratus,

    /// Answered in the legacy alloy format, so the execution results are leader reported.
    Alloy,
}

/// Block imported from the leader, normalized to the stratus-native format.
#[derive(Debug, Clone)]
pub struct ImportedBlock {
    /// Block in the stratus-native format.
    pub block: Block,

    /// Wire format the leader answered in.
    pub origin: BlockOrigin,
}

impl ImportedBlock {
    /// Converts a block with receipts answered in the legacy alloy format to the stratus-native
    /// format.
    fn from_alloy(block: ExternalBlock, mut receipts: Vec<ExternalReceipt>) -> anyhow::Result<Self> {
        let block_number = block.number();
        let block_timestamp = block.timestamp();
        let block_hash = block.hash();
        let block_gas_used = block.0.header.gas_used;

        let BlockTransactions::Full(mut transactions) = block.0.transactions else {
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

        transactions.sort_by_key(|tx| tx.transaction_index);
        receipts.sort_by_key(|receipt| receipt.transaction_index);

        for (position, (tx, receipt)) in transactions.iter().zip(receipts.iter()).enumerate() {
            let tx_index = tx
                .transaction_index
                .ok_or_else(|| anyhow!("block {block_number}: transaction at position {position} has no transaction index"))?;
            let receipt_index = receipt
                .0
                .transaction_index
                .ok_or_else(|| anyhow!("block {block_number}: receipt at position {position} has no transaction index"))?;
            if tx_index != receipt_index {
                bail!(
                    "block {block_number}: transaction/receipt pairing mismatch at position {position}: transaction index {tx_index} vs receipt index {receipt_index}"
                );
            }
        }

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

        let mut transactions_mined = Vec::with_capacity(transactions.len());
        let mut first_log_index = Index::ZERO;
        for (tx, receipt) in transactions.into_iter().zip(receipts.iter()) {
            let tx_input: TransactionInput = tx.try_into()?;
            let tx_index = receipt.0.transaction_index.ok_or(anyhow!("missing transaction index"))?;

            let logs: Vec<Log> = receipt.0.inner.logs().iter().cloned().map(Log::from).collect();

            let result = if receipt.is_success() {
                ExecutionResult::Success
            } else {
                ExecutionResult::new_reverted("reverted on the leader node".into())
            };

            let output = TransactionExecutionResult {
                result,
                output: Bytes::default(),
                gas_used: Gas::from(receipt.0.gas_used),
                deployed_contract_address: receipt.0.contract_address.map(Address::from),
                logs,
            };

            let execution_input = TransactionExecutionInput::create(
                &tx_input,
                BlockInfo {
                    number: block_number,
                    timestamp: block_timestamp.into(),
                },
            );

            let execution = TransactionExecution::new(tx_input.transaction_info, tx_input.signature, execution_input, output);

            let logs_len = execution.output.logs.len() as u64;
            transactions_mined.push(TransactionMined::from_execution(execution, block_hash, Index(tx_index as u64), first_log_index));
            first_log_index += Index(logs_len);
        }

        let mut block = Block::new(block_number, block_timestamp);
        block.header.hash = block_hash;
        block.header.size = Size::from(transactions_mined.len() as u64);
        block.header.gas_used = Gas::from(block_gas_used);
        block.transactions = transactions_mined;

        Ok(Self {
            block,
            origin: BlockOrigin::Alloy,
        })
    }

    /// Returns the block number.
    pub fn block_number(&self) -> BlockNumber {
        self.block.number()
    }

    /// Returns the number of transactions (and therefore of receipts).
    pub fn receipts_len(&self) -> usize {
        self.block.transactions.len()
    }
}

pub struct BlockWithReceiptsFetcher {
    pub chain: Arc<BlockchainClient>,
}

impl DataFetcher for BlockWithReceiptsFetcher {
    type FetchedType = BlockAndReceiptsResponse;
    type PostProcessType = ImportedBlock;

    async fn fetch(&self, block_number: BlockNumber) -> Self::FetchedType {
        let fetch_fn = |bn| {
            let chain = Arc::clone(&self.chain);
            async move { chain.fetch_block_and_receipts(bn).await }
        };

        fetch_with_retry(block_number, fetch_fn, "block and receipts").await
    }

    async fn post_process(&self, data: Self::FetchedType) -> anyhow::Result<Self::PostProcessType> {
        match data {
            BlockAndReceiptsResponse::Alloy(response) => ImportedBlock::from_alloy(response.block, response.receipts),
            BlockAndReceiptsResponse::Stratus(block) => {
                let block: Block = (*block).into();
                for window in block.transactions.windows(2) {
                    let tx_index = window[0].mined_data.index.0;
                    let next_tx_index = window[1].mined_data.index.0;
                    if tx_index + 1 != next_tx_index {
                        tracing::error!(tx_index, next_tx_index, "two consecutive transactions must have consecutive indices");
                    }
                }

                Ok(ImportedBlock {
                    block,
                    origin: BlockOrigin::Stratus,
                })
            }
        }
    }
}
