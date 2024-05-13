use std::sync::Arc;

use ethereum_types::BloomInput;
use keccak_hasher::KeccakHasher;
use nonempty::NonEmpty;
use tokio::sync::broadcast;

use crate::eth::evm::EvmExecutionResult;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockHeader;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalTransactionExecution;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Index;
use crate::eth::primitives::LocalTransactionExecution;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionInput;
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
    pub async fn mine_external(&self) -> anyhow::Result<Block> {
        // retrieve data
        let (external_block, txs) = read_external_block_and_executions(&self.storage).await?;
        let (local_txs, external_txs) = partition_transactions(txs);

        // validate
        if not(local_txs.is_empty()) {
            return log_and_err!("cannot mine external block because one of the transactions is not an external transaction");
        }

        // mine external transactions
        let mined_txs = mine_external_transactions(external_block.number(), external_txs)?;
        block_from_external(external_block, mined_txs)
    }

    /// Mine a block from an external block and local failed transactions.
    pub async fn mine_mixed(&self) -> anyhow::Result<Block> {
        // retrieve data
        let (external_block, txs) = read_external_block_and_executions(&self.storage).await?;
        let (local_txs, external_txs) = partition_transactions(txs);

        // mine external transactions
        let mined_txs = mine_external_transactions(external_block.number(), external_txs)?;
        let mut block = block_from_external(external_block, mined_txs)?;

        // mine local transactions
        for tx in local_txs {
            if tx.is_success() {
                return log_and_err!("cannot mine mixed block because one of the local execution is a success");
            }
            block.push_execution(tx.input, tx.result);
        }

        Ok(block)
    }

    /// Mine one block with a single transaction.
    /// Internally, it wraps the single transaction into a format suitable for `mine_with_many_transactions`,
    /// enabling consistent processing for both single and multiple transaction scenarios.
    ///
    /// TODO: remove
    pub async fn mine_with_one_transaction(&self, input: TransactionInput, evm_result: EvmExecutionResult) -> anyhow::Result<Block> {
        let transactions = NonEmpty::new((input, evm_result));
        self.mine_with_many_transactions(transactions).await
    }

    /// Mines a new block from one or more transactions.
    /// This is the core function for block creation, processing each transaction, generating the necessary logs,
    /// and finalizing the block. It is used both directly for multiple transactions and indirectly by `mine_with_one_transaction`.
    ///
    /// TODO: Future enhancements may include breaking down this method for improved readability and maintenance.
    pub async fn mine_with_many_transactions(&self, transactions: NonEmpty<(TransactionInput, EvmExecutionResult)>) -> anyhow::Result<Block> {
        // init block
        let number = self.storage.increment_block_number().await?;
        let block_timestamp = transactions
            .minimum_by(|(_, e1), (_, e2)| e1.execution.block_timestamp.cmp(&e2.execution.block_timestamp))
            .1
            .execution
            .block_timestamp;

        let mut block = Block::new(number, block_timestamp);
        block.transactions.reserve(transactions.len());

        // mine transactions and logs
        let mut log_index = Index::ZERO;
        for (tx_idx, (input, evm_result)) in transactions.into_iter().enumerate() {
            let transaction_index = Index::new(tx_idx as u64);
            // mine logs
            let mut mined_logs: Vec<LogMined> = Vec::with_capacity(evm_result.execution.logs.len());
            for mined_log in evm_result.execution.logs.clone() {
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
                execution: evm_result.execution,
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

    /// Persists a mined block to permanent storage and prepares new block.
    pub async fn commit(&self, block: Block) -> anyhow::Result<()> {
        let block_number = *block.number();

        // persist block
        self.storage.save_block_to_perm(block.clone()).await?;
        self.storage.set_mined_block_number(block_number).await?;

        // prepare new block to be mined
        self.storage.set_active_block_number(block_number.next()).await?;
        self.storage.reset_temp().await?;

        // notify
        let logs: Vec<LogMined> = block.transactions.iter().flat_map(|tx| &tx.logs).cloned().collect();
        for log in logs {
            let _ = self.notifier_logs.send(log);
        }
        let _ = self.notifier_blocks.send(block);

        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

async fn read_external_block_and_executions(storage: &StratusStorage) -> anyhow::Result<(ExternalBlock, Vec<TransactionExecution>)> {
    let block = match storage.temp.read_external_block().await {
        Ok(Some(block)) => block,
        Ok(None) => return log_and_err!("no active external block being re-executed"),
        Err(e) => return Err(e),
    };
    let txs = storage.temp.read_executions().await?;

    Ok((block, txs))
}

fn partition_transactions(txs: Vec<TransactionExecution>) -> (Vec<LocalTransactionExecution>, Vec<ExternalTransactionExecution>) {
    let mut local_txs = Vec::with_capacity(txs.len());
    let mut external_txs = Vec::with_capacity(txs.len());

    for tx in txs {
        match tx {
            TransactionExecution::Local(tx) => {
                local_txs.push(tx);
            }
            TransactionExecution::External(tx) => {
                external_txs.push(tx);
            }
        }
    }
    (local_txs, external_txs)
}

fn mine_external_transactions(block_number: BlockNumber, txs: Vec<ExternalTransactionExecution>) -> anyhow::Result<Vec<TransactionMined>> {
    let mut mined_txs = Vec::with_capacity(txs.len());
    for tx in txs {
        if tx.tx.block_number() != block_number {
            return log_and_err!("cannot mine external block because one of the transactions does not belong to the external block");
        }
        mined_txs.push(TransactionMined::from_external(tx.tx, tx.receipt, tx.result.execution)?);
    }
    Ok(mined_txs)
}

fn block_from_external(external_block: ExternalBlock, mined_txs: Vec<TransactionMined>) -> anyhow::Result<Block> {
    Ok(Block {
        header: BlockHeader::try_from(&external_block)?,
        transactions: mined_txs,
    })
}
