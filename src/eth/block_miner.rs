use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::sync::Condvar;
use std::thread;
use std::time::Duration;

use ethereum_types::BloomInput;
use keccak_hasher::KeccakHasher;
use nonempty::NonEmpty;
use tokio::runtime::Handle;
use tokio::sync::broadcast;

use super::Consensus;
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
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::StratusStorage;
use crate::ext::not;
use crate::log_and_err;

use super::transaction_relayer::ExternalRelayerClient;

pub struct BlockMiner {
    storage: Arc<StratusStorage>,

    /// Time duration between blocks.
    pub block_time: Option<Duration>,

    /// Broadcasts pending transactions events.
    pub notifier_pending_txs: broadcast::Sender<Hash>,

    /// Broadcasts new mined blocks events.
    pub notifier_blocks: broadcast::Sender<Block>,

    /// Broadcasts transaction logs events.
    pub notifier_logs: broadcast::Sender<LogMined>,

    /// External relayer client
    relayer_client: Option<ExternalRelayerClient>,

    /// Consensus logic.
    consensus: Option<Arc<Consensus>>,
}

impl BlockMiner {
    /// Creates a new [`BlockMiner`].
    pub fn new(storage: Arc<StratusStorage>, block_time: Option<Duration>, consensus: Option<Arc<Consensus>>, relayer_client: Option<ExternalRelayerClient>) -> Self {
        tracing::info!("starting block miner");
        Self {
            storage,
            block_time,
            notifier_pending_txs: broadcast::channel(u16::MAX as usize).0,
            notifier_blocks: broadcast::channel(u16::MAX as usize).0,
            notifier_logs: broadcast::channel(u16::MAX as usize).0,
            relayer_client,
            consensus
        }
    }

    /// Spawns a new thread that keep mining blocks in the specified interval.
    pub fn spawn_interval_miner(self: Arc<Self>) -> anyhow::Result<()> {
        // validate
        let Some(block_time) = self.block_time else {
            return log_and_err!("cannot spawn interval miner because it does not have a block time defined");
        };
        tracing::info!(block_time = %humantime::Duration::from(block_time), "spawning interval miner");

        // spawn scoped threads (tokio does not support scoped tasks)
        let pending_blocks = AtomicUsize::new(0);
        let pending_blocks_cvar = Condvar::new();

        thread::scope(|s| {
            // spawn miner
            let t_miner = thread::Builder::new().name("miner".into());
            let t_miner_tokio = Handle::current();
            let t_miner_pending_blocks = &pending_blocks;
            let t_miner_pending_blocks_cvar = &pending_blocks_cvar;
            t_miner
                .spawn_scoped(s, move || {
                    let _tokio_guard = t_miner_tokio.enter();
                    interval_miner::start(self, t_miner_pending_blocks, t_miner_pending_blocks_cvar);
                })
                .expect("spawning interval miner should not fail");

            // spawn ticker
            let t_ticker = thread::Builder::new().name("miner-ticker".into());
            let t_ticker_tokio = Handle::current();
            let t_ticker_pending_blocks = &pending_blocks;
            let t_ticker_pending_blocks_cvar = &pending_blocks_cvar;
            t_ticker
                .spawn_scoped(s, move || {
                    let _tokio_guard = t_ticker_tokio.enter();
                    interval_miner_ticker::start(block_time, t_ticker_pending_blocks, t_ticker_pending_blocks_cvar);
                })
                .expect("spawning interval miner ticker should not fail");
        });

        Ok(())
    }

    /// Checks if miner should run in interval miner mode.
    pub fn is_interval_miner_mode(&self) -> bool {
        self.block_time.is_some()
    }

    /// Checks if miner should run in automine mode.
    pub fn is_automine_mode(&self) -> bool {
        not(self.is_interval_miner_mode())
    }

    /// Persists a transaction execution.
    pub async fn save_execution(&self, tx_execution: TransactionExecution) -> anyhow::Result<()> {
        let tx_hash = tx_execution.hash();

        self.storage.save_execution(tx_execution.clone()).await?;
        if let Some(consensus) = &self.consensus {
            let execution = format!("{:?}", tx_execution.clone());
            consensus.sender.send(execution).await.unwrap();
        }
        let _ = self.notifier_pending_txs.send(tx_hash);

        Ok(())
    }

    /// Mines external block and external transactions.
    ///
    /// Local transactions are not allowed to be part of the block.
    pub async fn mine_external(&self) -> anyhow::Result<Block> {
        tracing::debug!("mining external block");

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
        tracing::debug!("mining external mixed block");

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
        tracing::debug!("mining local block");

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
        tracing::debug!(number = %block.number(), transactions_len = %block.transactions.len(), "commiting block");

        let block_number = *block.number();

        // persist block
        self.storage.save_block(block.clone()).await?;
        self.storage.set_mined_block_number(block_number).await?;

        if let Some(relayer) = &self.relayer_client {
            relayer.send_to_relayer(block.clone()).await?; // TODO: do this through a channel to another task
        }

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
mod interval_miner {
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use std::sync::Condvar;
    use std::sync::Mutex;

    use tokio::runtime::Handle;

    use crate::eth::BlockMiner;

    pub fn start(miner: Arc<BlockMiner>, pending_blocks: &AtomicUsize, pending_blocks_cvar: &Condvar) {
        let tokio = Handle::current();
        let cvar_mutex = Mutex::new(());

        loop {
            let pending = pending_blocks
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| Some(n.saturating_sub(1)))
                .unwrap();
            if pending > 0 {
                tracing::info!(%pending, "interval mining block");
                tokio.block_on(mine_and_commit(&miner));
            } else {
                tracing::debug!(%pending, "waiting for block interval");
                let _ = pending_blocks_cvar.wait(cvar_mutex.lock().unwrap());
            }
        }
    }

    #[inline(always)]
    async fn mine_and_commit(miner: &BlockMiner) {
        // mine
        let block = loop {
            match miner.mine_local().await {
                Ok(block) => break block,
                Err(e) => {
                    tracing::error!(reason = ?e, "failed to mine block");
                }
            }
        };

        // commit
        loop {
            match miner.commit(block.clone()).await {
                Ok(_) => break,
                Err(e) => {
                    tracing::error!(reason = ?e, "failed to commit block");
                    continue;
                }
            }
        }
    }
}

mod interval_miner_ticker {
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::sync::Condvar;
    use std::thread;
    use std::time::Duration;

    use chrono::Timelike;
    use chrono::Utc;
    use tokio::runtime::Handle;

    pub fn start(block_time: Duration, pending_blocks: &AtomicUsize, pending_blocks_cvar: &Condvar) {
        let tokio = Handle::current();

        // sync to next second
        let next_second = (Utc::now() + Duration::from_secs(1)).with_nanosecond(0).unwrap();
        thread::sleep((next_second - Utc::now()).to_std().unwrap());

        // prepare ticker
        let mut ticker = tokio::time::interval(block_time);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Burst);

        // keep ticking
        tokio.block_on(async move {
            ticker.tick().await;
            loop {
                let _ = ticker.tick().await;
                let _ = pending_blocks.fetch_add(1, Ordering::SeqCst);
                pending_blocks_cvar.notify_one();
            }
        });
    }
}
