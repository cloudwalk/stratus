use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;

use keccak_hasher::KeccakHasher;
use nonempty::NonEmpty;
use tokio::sync::broadcast;
use tracing::Span;

use crate::eth::miner::MinerMode;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockHeader;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalTransactionExecution;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Index;
use crate::eth::primitives::LocalTransactionExecution;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::Size;
use crate::eth::primitives::StratusError;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::StratusStorage;
use crate::ext::not;
use crate::ext::spawn_thread;
use crate::ext::DisplayExt;
use crate::infra::tracing::SpanExt;
use crate::log_and_err;

pub struct Miner {
    pub locks: MinerLocks,

    storage: Arc<StratusStorage>,

    /// Mode the block miner is running.
    pub mode: MinerMode,

    /// Broadcasts pending transactions events.
    pub notifier_pending_txs: broadcast::Sender<TransactionExecution>,

    /// Broadcasts new mined blocks events.
    pub notifier_blocks: broadcast::Sender<Block>,

    /// Broadcasts transaction logs events.
    pub notifier_logs: broadcast::Sender<LogMined>,
}

/// Locks used in operations that mutate state.
#[derive(Default)]
pub struct MinerLocks {
    save_execution: Mutex<()>,
    pub mine_and_commit: Mutex<()>,
    mine: Mutex<()>,
    commit: Mutex<()>,
}

impl Miner {
    /// Creates a new [`BlockMiner`].
    pub fn new(storage: Arc<StratusStorage>, mode: MinerMode) -> Self {
        tracing::info!(?mode, "creating block miner");
        Self {
            locks: MinerLocks::default(),
            storage,
            mode,
            notifier_pending_txs: broadcast::channel(u16::MAX as usize).0,
            notifier_blocks: broadcast::channel(u16::MAX as usize).0,
            notifier_logs: broadcast::channel(u16::MAX as usize).0,
        }
    }

    /// Spawns a new thread that keep mining blocks in the specified interval.
    pub fn spawn_interval_miner(self: Arc<Self>) -> anyhow::Result<()> {
        // validate
        let MinerMode::Interval(block_time) = self.mode else {
            return log_and_err!("cannot spawn interval miner because it does not have a block time defined");
        };
        tracing::info!(block_time = %block_time.to_string_ext(), "spawning interval miner");

        // spawn miner and ticker
        let (ticks_tx, ticks_rx) = mpsc::channel();
        spawn_thread("miner-miner", move || interval_miner::run(Arc::clone(&self), ticks_rx));
        spawn_thread("miner-ticker", move || interval_miner_ticker::run(block_time, ticks_tx));
        Ok(())
    }

    /// Returns the mode the miner is running.
    pub fn mode(&self) -> &MinerMode {
        &self.mode
    }

    /// Persists a transaction execution.
    #[tracing::instrument(name = "miner::save_execution", skip_all, fields(tx_hash))]
    pub fn save_execution(&self, tx_execution: TransactionExecution) -> Result<(), StratusError> {
        Span::with(|s| {
            s.rec_str("tx_hash", &tx_execution.hash());
        });

        // if automine is enabled, only one transaction can enter the block at time.
        let _save_execution_lock = if self.mode.is_automine() {
            Some(self.locks.save_execution.lock().unwrap())
        } else {
            None
        };

        // save execution to temporary storage
        self.storage.save_execution(tx_execution.clone())?;

        // if automine is enabled, automatically mines a block
        let _ = self.notifier_pending_txs.send(tx_execution);
        if self.mode.is_automine() {
            self.mine_local_and_commit()?;
        }

        Ok(())
    }

    /// Same as [`Self::mine_external`], but automatically commits the block instead of returning it.
    pub fn mine_external_and_commit(&self) -> anyhow::Result<()> {
        let _mine_and_commit_lock = self.locks.mine_and_commit.lock().unwrap();
        tracing::info!("miner acquired mine and commit lock for external block");

        let block = self.mine_external()?;
        self.commit(block)
    }

    /// Mines external block and external transactions.
    ///
    /// Local transactions are not allowed to be part of the block.
    #[tracing::instrument(name = "miner::mine_external", skip_all, fields(block_number))]
    pub fn mine_external(&self) -> anyhow::Result<Block> {
        tracing::debug!("mining external block");

        // lock
        let _mine_lock = self.locks.mine.lock().unwrap();
        tracing::info!("miner acquired mine lock for external block");

        // mine
        let block = self.storage.finish_pending_block()?;
        let (local_txs, external_txs) = block.split_transactions();

        // validate
        let Some(external_block) = block.external_block else {
            return log_and_err!("failed to mine external block because there is no external block being reexecuted");
        };
        if not(local_txs.is_empty()) {
            return log_and_err!("failed to mine external block because one of the transactions is a local transaction");
        }

        // mine external transactions
        let mined_txs = mine_external_transactions(block.number, external_txs)?;
        let block = block_from_external(external_block, mined_txs);

        block.inspect(|block| Span::with(|s| s.rec_str("block_number", &block.number())))
    }

    /// Same as [`Self::mine_external_mixed`], but automatically commits the block instead of returning it.
    pub fn mine_external_mixed_and_commit(&self) -> anyhow::Result<()> {
        let _mine_and_commit_lock = self.locks.mine_and_commit.lock().unwrap();
        tracing::info!("miner acquired mine and commit lock for external mixed local block");

        let block = self.mine_external_mixed()?;
        self.commit(block)
    }

    /// Mines external block and external transactions.
    ///
    /// Local transactions are allowed to be part of the block if failed, but not succesful ones.
    #[tracing::instrument(name = "miner::mine_external_mixed", skip_all, fields(block_number))]
    pub fn mine_external_mixed(&self) -> anyhow::Result<Block> {
        tracing::debug!("mining external mixed block");

        // lock
        let _mine_lock = self.locks.mine.lock().unwrap();
        tracing::info!("miner acquired mining lock for external mixed block");

        // mine
        let block = self.storage.finish_pending_block()?;
        let (local_txs, external_txs) = block.split_transactions();

        // validate
        let Some(external_block) = block.external_block else {
            return log_and_err!("failed to mine external mixed block because there is no external block being reexecuted");
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

        Span::with(|s| s.rec_str("block_number", &block.number()));

        Ok(block)
    }

    /// Same as [`Self::mine_local`], but automatically commits the block instead of returning it.
    /// mainly used when is_automine is enabled.
    pub fn mine_local_and_commit(&self) -> anyhow::Result<()> {
        let _mine_and_commit_lock = self.locks.mine_and_commit.lock().unwrap();
        tracing::info!("miner acquired mine and commit lock for local block");

        let block = self.mine_local()?;
        self.commit(block)
    }

    /// Mines local transactions.
    ///
    /// External transactions are not allowed to be part of the block.
    #[tracing::instrument(name = "miner::mine_local", skip_all, fields(block_number))]
    pub fn mine_local(&self) -> anyhow::Result<Block> {
        tracing::debug!("mining local block");

        // lock
        let _mine_lock = self.locks.mine.lock().unwrap();
        tracing::info!("miner acquired mining lock for local block");

        // mine
        let block = self.storage.finish_pending_block()?;
        let (local_txs, external_txs) = block.split_transactions();

        // validate
        if not(external_txs.is_empty()) {
            return log_and_err!("failed to mine local block because one of the transactions is an external transaction");
        }

        // mine local transactions
        let block = match NonEmpty::from_vec(local_txs) {
            Some(local_txs) => block_from_local(block.number, local_txs),
            None => Ok(Block::new_at_now(block.number)),
        };

        block.inspect(|block| Span::with(|s| s.rec_str("block_number", &block.number())))
    }

    /// Persists a mined block to permanent storage and prepares new block.
    #[tracing::instrument(name = "miner::commit", skip_all, fields(block_number))]
    pub fn commit(&self, block: Block) -> anyhow::Result<()> {
        Span::with(|s| s.rec_str("block_number", &block.number()));
        tracing::info!(block_number = %block.number(), transactions_len = %block.transactions.len(), "commiting block");

        // lock
        let _commit_lock = self.locks.commit.lock().unwrap();
        tracing::info!(block_number = %block.number(), "miner acquired commit lock");

        // extract fields to use in notifications
        let block_number = block.number();
        let block_logs: Vec<LogMined> = block.transactions.iter().flat_map(|tx| &tx.logs).cloned().collect();

        self.storage.save_block(block.clone())?;
        self.storage.set_mined_block_number(block_number)?;

        // notify
        for log in block_logs {
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
        mined_txs.push(TransactionMined::from_external(tx.tx, tx.receipt, tx.evm_execution.execution)?);
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
    block.header.size = Size::from(txs.len() as u64);

    // mine transactions and logs
    let mut log_index = Index::ZERO;
    for (tx_idx, tx) in txs.into_iter().enumerate() {
        let transaction_index = Index::new(tx_idx as u64);
        // mine logs
        let mut mined_logs: Vec<LogMined> = Vec::with_capacity(tx.result.execution.logs.len());
        for mined_log in tx.result.execution.logs.clone() {
            // calculate bloom
            block.header.bloom.accrue_log(&mined_log);

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

    // TODO: calculate state_root, receipts_root and parent_hash
    Ok(block)
}

// -----------------------------------------------------------------------------
// Miner
// -----------------------------------------------------------------------------
mod interval_miner {
    use std::sync::mpsc;
    use std::sync::mpsc::RecvTimeoutError;
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::time::Instant;

    use crate::eth::miner::Miner;
    use crate::ext::not;
    use crate::infra::tracing::warn_task_rx_closed;
    use crate::GlobalState;

    pub fn run(miner: Arc<Miner>, ticks_rx: mpsc::Receiver<Instant>) {
        const TASK_NAME: &str = "interval-miner-ticker";

        loop {
            if GlobalState::is_shutdown_warn(TASK_NAME) {
                break;
            }

            let tick = match ticks_rx.recv_timeout(Duration::from_secs(2)) {
                Ok(tick) => tick,
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {
                    tracing::warn!(timeout = "2s", "timeout reading miner channel, expected 1 block per second");
                    continue;
                }
            };

            if not(GlobalState::is_miner_enabled()) {
                tracing::warn!("skipping mining block because block mining is disabled");
                continue;
            }

            // mine
            tracing::info!(lag_us = %tick.elapsed().as_micros(), "interval mining block");
            mine_and_commit(&miner);
        }
        warn_task_rx_closed(TASK_NAME);
    }

    #[inline(always)]
    fn mine_and_commit(miner: &Miner) {
        let _mine_and_commit_lock = miner.locks.mine_and_commit.lock().unwrap();
        tracing::info!("miner acquired mine and commit lock for interval local block");

        // mine
        let block = loop {
            match miner.mine_local() {
                Ok(block) => break block,
                Err(e) => {
                    tracing::error!(reason = ?e, "failed to mine block");
                }
            }
        };

        // commit
        loop {
            match miner.commit(block.clone()) {
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
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use chrono::Timelike;
    use chrono::Utc;
    use tokio::runtime::Handle;
    use tokio::time::Instant;

    use crate::infra::tracing::warn_task_rx_closed;
    use crate::GlobalState;

    pub fn run(block_time: Duration, ticks_tx: mpsc::Sender<Instant>) {
        const TASK_NAME: &str = "interval-miner-ticker";

        // sync to next second
        let next_second = (Utc::now() + Duration::from_secs(1)).with_nanosecond(0).unwrap();
        thread::sleep((next_second - Utc::now()).to_std().unwrap());

        // prepare ticker
        let mut ticker = tokio::time::interval(block_time);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Burst);

        // keep ticking
        let runtime = Handle::current();
        runtime.block_on(async {
            ticker.tick().await;
            loop {
                if GlobalState::is_shutdown_warn(TASK_NAME) {
                    println!("e");
                    return;
                }

                let tick = ticker.tick().await;
                if ticks_tx.send(tick).is_err() {
                    warn_task_rx_closed(TASK_NAME);
                    break;
                };
            }
        });
    }
}
