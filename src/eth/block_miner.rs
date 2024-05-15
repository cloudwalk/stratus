use std::sync::Arc;
use std::thread;
use std::time::Duration;

use ethereum_types::BloomInput;
use keccak_hasher::KeccakHasher;
use nonempty::NonEmpty;
use tokio::sync::broadcast;

use crate::eth::primitives::Block;
use crate::eth::primitives::BlockHeader;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalTransactionExecution;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Index;
use crate::eth::primitives::LocalTransactionExecution;
use crate::eth::primitives::LogMined;
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
        tracing::info!("starting block miner");
        Self {
            storage,
            notifier_blocks: broadcast::channel(u16::MAX as usize).0,
            notifier_logs: broadcast::channel(u16::MAX as usize).0,
        }
    }

    pub fn spawn_interval_miner(self: Arc<Self>, block_time: Duration) {
        tracing::info!(block_time = %humantime::Duration::from(block_time), "spawning interval miner");

        let t = thread::Builder::new().name("interval-miner".into());
        t.spawn(move || interval_miner(self, block_time))
            .expect("spawning interval miner should not fail");
    }

    /// Mines external block and external transactions.
    ///
    /// Local transactions are not allowed to be part of the block.
    pub async fn mine_external(&self) -> anyhow::Result<Block> {
        let block = self.storage.finish_block().await?;
        let (local_txs, external_txs) = block.split_transactions();

        // validate
        let Some(external_block) = block.external_block else {
            return log_and_err!("failed to mine external block because there is no external block being re-executed");
        };
        if not(local_txs.is_empty()) {
            return log_and_err!("failed to mine external block because one of the transactions is a local transaction");
        }

        // mine external transactions
        let mined_txs = mine_external_transactions(block.number, external_txs)?;
        block_from_external(external_block, mined_txs)
    }

    /// Same as [`Self::mine_external`], but automatically commits the block instead of returning it.
    pub async fn mine_external_and_commit(&self) -> anyhow::Result<()> {
        let block = self.mine_external().await?;
        self.commit(block).await
    }

    /// Mines external block and external transactions.
    ///
    /// Local transactions are allowed to be part of the block if failed, but not succesful ones.
    pub async fn mine_external_mixed(&self) -> anyhow::Result<Block> {
        let block = self.storage.finish_block().await?;
        let (local_txs, external_txs) = block.split_transactions();

        // validate
        let Some(external_block) = block.external_block else {
            return log_and_err!("failed to mine mixed block because there is no external block being re-executed");
        };

        // mine external transactions
        let mined_txs = mine_external_transactions(block.number, external_txs)?;
        let mut block = block_from_external(external_block, mined_txs)?;

        // mine local transactions
        for tx in local_txs {
            if tx.is_success() {
                return log_and_err!("failed to mine mixed block because one of the local execution is a success");
            }
            block.push_execution(tx.input, tx.result);
        }

        Ok(block)
    }

    /// Same as [`Self::mine_external_mixed`], but automatically commits the block instead of returning it.
    pub async fn mine_external_mixed_and_commit(&self) -> anyhow::Result<()> {
        let block = self.mine_external_mixed().await?;
        self.commit(block).await
    }

    /// Mines local transactions.
    ///
    /// External transactions are not allowed to be part of the block.
    pub async fn mine_local(&self) -> anyhow::Result<Block> {
        let block = self.storage.finish_block().await?;
        let (local_txs, external_txs) = block.split_transactions();

        // validate
        if not(external_txs.is_empty()) {
            return log_and_err!("failed to mine local block because one of the transactions is an external transaction");
        }

        // mine local transactions
        match NonEmpty::from_vec(local_txs) {
            Some(local_txs) => block_from_local(block.number, local_txs),
            None => Ok(Block::new_at_now(block.number)),
        }
    }

    /// Same as [`Self::mine_local`], but automatically commits the block instead of returning it.
    pub async fn mine_local_and_commit(&self) -> anyhow::Result<()> {
        let block = self.mine_local().await?;
        self.commit(block).await
    }

    /// Persists a mined block to permanent storage and prepares new block.
    pub async fn commit(&self, block: Block) -> anyhow::Result<()> {
        let block_number = *block.number();

        // persist block
        self.storage.save_block(block.clone()).await?;
        self.storage.set_mined_block_number(block_number).await?;

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

fn mine_external_transactions(block_number: BlockNumber, txs: Vec<ExternalTransactionExecution>) -> anyhow::Result<Vec<TransactionMined>> {
    let mut mined_txs = Vec::with_capacity(txs.len());
    for tx in txs {
        if tx.tx.block_number() != block_number {
            return log_and_err!("failed to mine external block because one of the transactions does not belong to the external block");
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

pub fn block_from_local(number: BlockNumber, txs: NonEmpty<LocalTransactionExecution>) -> anyhow::Result<Block> {
    // init block
    let block_timestamp = txs
        .minimum_by(|tx1, tx2| tx1.result.execution.block_timestamp.cmp(&tx2.result.execution.block_timestamp))
        .result
        .execution
        .block_timestamp;

    let mut block = Block::new(number, block_timestamp);
    block.transactions.reserve(txs.len());

    // mine transactions and logs
    let mut log_index = Index::ZERO;
    for (tx_idx, tx) in txs.into_iter().enumerate() {
        let transaction_index = Index::new(tx_idx as u64);
        // mine logs
        let mut mined_logs: Vec<LogMined> = Vec::with_capacity(tx.result.execution.logs.len());
        for mined_log in tx.result.execution.logs.clone() {
            // calculate bloom
            block.header.bloom.accrue(BloomInput::Raw(mined_log.address.as_ref()));
            for topic in mined_log.topics().into_iter() {
                block.header.bloom.accrue(BloomInput::Raw(topic.as_ref()));
            }

            // mine log
            let mined_log = LogMined {
                log: mined_log,
                transaction_hash: tx.input.hash,
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
            input: tx.input,
            execution: tx.result.execution,
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

// -----------------------------------------------------------------------------
// Miner
// -----------------------------------------------------------------------------
fn interval_miner(miner: Arc<BlockMiner>, block_time: Duration) {
    loop {
        thread::sleep(block_time);
        tracing::info!("mining block");

        // mine
        let block = match futures::executor::block_on(miner.mine_local()) {
            Ok(block) => block,
            Err(e) => {
                tracing::error!(reason = ?e, "failed to mine block");
                continue;
            }
        };

        // commit
        loop {
            match futures::executor::block_on(miner.commit(block.clone())) {
                Ok(_) => break,
                Err(e) => {
                    tracing::error!(reason = ?e, "failed to commit block");
                    continue;
                }
            }
        }
    }
}
