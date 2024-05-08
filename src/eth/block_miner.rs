use std::sync::Arc;

use ethereum_types::BloomInput;
use keccak_hasher::KeccakHasher;
use nonempty::NonEmpty;
use tokio::sync::broadcast;

use crate::eth::primitives::Block;
use crate::eth::primitives::BlockHeader;
use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Index;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::TransactionKind;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::StratusStorage;
use crate::ext::not;
use crate::log_and_err;

pub struct BlockMiner {
    storage: Arc<StratusStorage>,

    /// Broadcasts new mined blocks events.
    pub notifier_blocks: broadcast::Sender<Block>,

    /// Broadcasts transaction logs events.
    pub notifier_logs: broadcast::Sender<LogMined>,
}

impl BlockMiner {
    /// Creates a new [`BlockMiner`].
    pub fn new(storage: Arc<StratusStorage>) -> Self {
        tracing::info!("creating block miner");
        Self {
            storage,
            notifier_blocks: broadcast::channel(u16::MAX as usize).0,
            notifier_logs: broadcast::channel(u16::MAX as usize).0,
        }
    }

    /// Mine a block with no transactions.
    pub async fn mine_empty(&self) -> anyhow::Result<Block> {
        let number = self.storage.increment_block_number().await?;
        Ok(Block::new_at_now(number))
    }

    /// Mine a block from an external block.
    ///
    /// TODO: external_block must come from storage.
    /// TODO: validate if transactions really belong to the specified block.
    pub async fn mine_external(&self, external_block: &ExternalBlock) -> anyhow::Result<Block> {
        // TODO: draining executions must be atomic instead of 2 calls
        let txs = self.storage.temp.read_executions().await;
        self.storage.temp.reset_executions().await;

        // mine external transactions.
        // fails if finds a transaction that is not external.
        let mut mined_txs = Vec::with_capacity(txs.len());
        for tx in txs {
            let TransactionKind::External(external_tx, external_receipt) = tx.kind else {
                return log_and_err!("cannot mine external block because one of the transactions is not an external transaction");
            };
            let mined_tx = TransactionMined::from_external(external_tx, external_receipt, tx.execution)?;
            mined_txs.push(mined_tx);
        }

        Ok(Block {
            header: BlockHeader::try_from(external_block)?,
            transactions: mined_txs,
        })
    }

    /// Mine a block from an external block and local failed transactions.
    ///
    /// TODO: external_block must come from storage.
    /// TODO: validate if transactions really belong to the specified block.
    pub async fn mine_mixed(&self, external_block: &ExternalBlock) -> anyhow::Result<Block> {
        // TODO: draining executions must be atomic instead of 2 calls
        let txs = self.storage.temp.read_executions().await;
        self.storage.temp.reset_executions().await;

        let mut mined_txs = Vec::with_capacity(txs.len());
        let mut failed_txs = Vec::new();

        // mine external transactions
        for tx in txs {
            match tx.kind {
                TransactionKind::External(external_tx, external_receipt) => {
                    let mined_tx = TransactionMined::from_external(external_tx, external_receipt, tx.execution)?;
                    mined_txs.push(mined_tx);
                }
                TransactionKind::Local(tx_input) => {
                    failed_txs.push((tx_input, tx.execution));
                }
            }
        }

        let mut block = Block {
            header: BlockHeader::try_from(external_block)?,
            transactions: mined_txs,
        };

        // mine failed transactions
        // fails if finds a local transaction that is not a failure
        for (failed_tx_input, failed_tx_execution) in failed_txs {
            if failed_tx_execution.is_success() {
                return log_and_err!("cannot mine mixed block because one of the local execution is not a failure");
            }
            block.push_execution(failed_tx_input, failed_tx_execution);
        }

        Ok(block)
    }

    /// Mine one block with a single transaction.
    /// Internally, it wraps the single transaction into a format suitable for `mine_with_many_transactions`,
    /// enabling consistent processing for both single and multiple transaction scenarios.
    ///
    /// TODO: remove
    pub async fn mine_with_one_transaction(&self, input: TransactionInput, execution: EvmExecution) -> anyhow::Result<Block> {
        let transactions = NonEmpty::new((input, execution));
        self.mine_with_many_transactions(transactions).await
    }

    /// Mines a new block from one or more transactions.
    /// This is the core function for block creation, processing each transaction, generating the necessary logs,
    /// and finalizing the block. It is used both directly for multiple transactions and indirectly by `mine_with_one_transaction`.
    ///
    /// TODO: Future enhancements may include breaking down this method for improved readability and maintenance.
    pub async fn mine_with_many_transactions(&self, transactions: NonEmpty<(TransactionInput, EvmExecution)>) -> anyhow::Result<Block> {
        // init block
        let number = self.storage.increment_block_number().await?;
        let block_timestamp = transactions
            .minimum_by(|(_, e1), (_, e2)| e1.block_timestamp.cmp(&e2.block_timestamp))
            .1
            .block_timestamp;

        let mut block = Block::new(number, block_timestamp);
        block.transactions.reserve(transactions.len());

        // mine transactions and logs
        let mut log_index = Index::ZERO;
        for (tx_idx, (input, execution)) in transactions.into_iter().enumerate() {
            let transaction_index = Index::new(tx_idx as u64);
            // mine logs
            let mut mined_logs: Vec<LogMined> = Vec::with_capacity(execution.logs.len());
            for mined_log in execution.logs.clone() {
                // calculate bloom
                block.header.bloom.accrue(BloomInput::Raw(mined_log.address.as_ref()));
                for topic in mined_log.topics().into_iter() {
                    block.header.bloom.accrue(BloomInput::Raw(topic.as_ref()));
                }

                // mine log
                let mined_log = LogMined {
                    log: mined_log,
                    transaction_hash: input.hash,
                    transaction_index,
                    log_index,
                    block_number: block.header.number,
                    block_hash: block.header.hash,
                };
                mined_logs.push(mined_log);

                // increment log index
                log_index = log_index + Index::ONE;
            }

            // mine transaction
            let mined_transaction = TransactionMined {
                input,
                execution,
                transaction_index,
                block_number: block.header.number,
                block_hash: block.header.hash,
                logs: mined_logs,
            };

            // add transaction to block
            block.transactions.push(mined_transaction);
        }

        // calculate transactions hash
        if not(block.transactions.is_empty()) {
            let transactions_hashes: Vec<&Hash> = block.transactions.iter().map(|x| &x.input.hash).collect();
            block.header.transactions_root = triehash::ordered_trie_root::<KeccakHasher, _>(transactions_hashes).into();
        }

        // calculate final block hash

        // replicate calculated block hash from header to transactions and logs
        for transaction in block.transactions.iter_mut() {
            transaction.block_hash = block.header.hash;
            for log in transaction.logs.iter_mut() {
                log.block_hash = block.header.hash;
            }
        }

        // TODO: calculate size, state_root, receipts_root, parent_hash

        Ok(block)
    }

    /// Persists a mined block to permanent storage.
    pub async fn commit(&self, block: Block) -> anyhow::Result<()> {
        let block_number = *block.number();

        // persist
        self.storage.commit_to_perm(block.clone()).await?;
        self.storage.set_mined_block_number(block_number).await?; // TODO: commit_to_perm should set the miner block number

        // notify
        let logs: Vec<LogMined> = block.transactions.iter().flat_map(|tx| &tx.logs).cloned().collect();
        for log in logs {
            let _ = self.notifier_logs.send(log);
        }
        let _ = self.notifier_blocks.send(block);

        Ok(())
    }
}
