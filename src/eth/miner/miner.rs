use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use std::time::Duration;

use anyhow::anyhow;
use itertools::Itertools;
use keccak_hasher::KeccakHasher;
use tokio::sync::broadcast;
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
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
use crate::eth::primitives::UnixTime;
use crate::eth::storage::StratusStorage;
use crate::ext::not;
use crate::ext::DisplayExt;
use crate::ext::MutexExt;
use crate::ext::MutexResultExt;
use crate::globals::STRATUS_SHUTDOWN_SIGNAL;
use crate::infra::tracing::SpanExt;
use crate::log_and_err;

cfg_if::cfg_if! {
    if #[cfg(feature = "tracing")] {
        use tracing::field;
        use tracing::info_span;
    }
}

pub struct Miner {
    pub locks: MinerLocks,

    storage: Arc<StratusStorage>,

    /// Miner is enabled by default, but can be disabled.
    is_paused: AtomicBool,

    /// Mode the block miner is running.
    mode: RwLock<MinerMode>,

    /// Broadcasts pending transactions events.
    pub notifier_pending_txs: broadcast::Sender<Hash>,

    /// Broadcasts new mined blocks events.
    pub notifier_blocks: broadcast::Sender<BlockHeader>,

    /// Broadcasts transaction logs events.
    pub notifier_logs: broadcast::Sender<LogMined>,

    // -------------------------------------------------------------------------
    // Shutdown
    // -------------------------------------------------------------------------
    /// Signal sent to tasks to shutdown.
    shutdown_signal: Mutex<CancellationToken>,

    /// Spawned tasks for interval miner, can be used to await for complete shutdown.
    interval_joinset: AsyncMutex<Option<JoinSet<()>>>,
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
    pub fn new(storage: Arc<StratusStorage>, mode: MinerMode) -> Self {
        tracing::info!(?mode, "creating block miner");
        Self {
            locks: MinerLocks::default(),
            storage,
            is_paused: AtomicBool::new(false),
            mode: mode.into(),
            notifier_pending_txs: broadcast::channel(u16::MAX as usize).0,
            notifier_blocks: broadcast::channel(u16::MAX as usize).0,
            notifier_logs: broadcast::channel(u16::MAX as usize).0,
            shutdown_signal: Mutex::new(STRATUS_SHUTDOWN_SIGNAL.child_token()),
            interval_joinset: AsyncMutex::new(None),
        }
    }

    /// Spawns a new thread that keep mining blocks in the specified interval.
    ///
    /// Also unpauses `Miner` if it was paused.
    pub async fn start_interval_mining(self: &Arc<Self>, block_time: Duration) {
        if self.is_interval_miner_running() {
            tracing::warn!(block_time = ?block_time.to_string_ext(), "tried to start interval mining, but it's already running, skipping");
            return;
        };

        tracing::info!(block_time = ?block_time.to_string_ext(), "spawning interval miner");
        self.set_mode(MinerMode::Interval(block_time));
        self.unpause();

        // spawn miner and ticker
        let (ticks_tx, ticks_rx) = mpsc::channel();
        let new_shutdown_signal = STRATUS_SHUTDOWN_SIGNAL.child_token();
        let mut joinset = JoinSet::new();

        joinset.spawn_blocking({
            let shutdown = new_shutdown_signal.clone();
            let miner_clone = Arc::clone(self);
            || interval_miner::run(miner_clone, ticks_rx, shutdown)
        });

        joinset.spawn(interval_miner_ticker::run(block_time, ticks_tx, new_shutdown_signal.clone()));

        *self.shutdown_signal.lock_or_clear("setting up shutdown signal for interval miner") = new_shutdown_signal;
        *self.interval_joinset.lock().await = Some(joinset);
    }

    /// Shuts down interval miner, set miner mode to External.
    pub async fn switch_to_external_mode(self: &Arc<Self>) {
        if self.mode().is_external() {
            tracing::warn!("trying to change mode to external, but it's already set, skipping");
            return;
        }
        self.shutdown_and_wait().await;
        self.unpause();
    }

    // Unpause interval miner (if in interval mode)
    pub fn unpause(&self) {
        self.is_paused.store(false, Ordering::Relaxed);
    }

    // Pause interval miner (if in interval mode)
    pub fn pause(&self) {
        self.is_paused.store(true, Ordering::Relaxed);
    }

    // Whether or not interval miner is paused (means nothing if not in interval mode)
    pub fn is_paused(&self) -> bool {
        self.is_paused.load(Ordering::Relaxed)
    }

    pub fn mode(&self) -> MinerMode {
        *self.mode.read().unwrap_or_else(|poison_error| {
            tracing::error!("miner mode read lock was poisoned");
            self.mode.clear_poison();
            poison_error.into_inner()
        })
    }

    fn set_mode(&self, new_mode: MinerMode) {
        *self.mode.write().unwrap_or_else(|poison_error| {
            tracing::error!("miner mode write lock was poisoned");
            self.mode.clear_poison();
            poison_error.into_inner()
        }) = new_mode;
    }

    pub fn is_interval_miner_running(&self) -> bool {
        match self.interval_joinset.try_lock() {
            // check if the joinset of tasks has futures running
            Ok(joinset) => joinset.is_some(),
            // if the joinset is locked, it's either trying to shutdown or turning on, so yes
            Err(_) => true,
        }
    }

    /// Shutdown if miner is interval miner.
    async fn shutdown_and_wait(&self) {
        // Note: we are intentionally holding this mutex till the end of the function, so that
        // subsequent calls wait for the first to finish, and `is_interval_miner_running` works too
        let mut joinset_lock = self.interval_joinset.lock().await;

        let Some(mut joinset) = joinset_lock.take() else {
            return;
        };

        tracing::warn!("Shutting down interval miner to switch to external mode");

        self.shutdown_signal.lock_or_clear("sending shutdown signal to interval miner").cancel();

        // wait for all tasks to end
        while let Some(result) = joinset.join_next().await {
            if let Err(e) = result {
                tracing::error!(reason = ?e, "miner task failed");
            }
        }
    }

    /// Persists a transaction execution.
    pub fn save_execution(&self, tx_execution: TransactionExecution, check_conflicts: bool) -> Result<(), StratusError> {
        let tx_hash = tx_execution.hash();

        // track
        #[cfg(feature = "tracing")]
        let _span = info_span!("miner::save_execution", %tx_hash).entered();

        // Check if automine is enabled
        let is_automine = self.mode().is_automine();

        // if automine is enabled, only one transaction can enter the block at a time.
        let _save_execution_lock = if is_automine {
            Some(self.locks.save_execution.lock().map_lock_error("save_execution")?)
        } else {
            None
        };

        // save execution to temporary storage
        self.storage.save_execution(tx_execution, check_conflicts)?;

        // notify
        let _ = self.notifier_pending_txs.send(tx_hash);

        // if automine is enabled, automatically mines a block
        if is_automine {
            self.mine_local_and_commit()?;
        }

        Ok(())
    }

    /// Same as [`Self::mine_external`], but automatically commits the block instead of returning it.
    pub fn mine_external_and_commit(&self) -> anyhow::Result<()> {
        let _mine_and_commit_lock = self.locks.mine_and_commit.lock().map_lock_error("mine_external_and_commit")?;

        let block = self.mine_external()?;
        self.commit(block)
    }

    /// Mines external block and external transactions.
    ///
    /// Local transactions are not allowed to be part of the block.
    pub fn mine_external(&self) -> anyhow::Result<Block> {
        // track
        #[cfg(feature = "tracing")]
        let _span = info_span!("miner::mine_external", block_number = field::Empty).entered();

        // lock
        let _mine_lock = self.locks.mine.lock().map_lock_error("mine_external")?;

        // mine block
        let block = self.storage.finish_pending_block()?;
        Span::with(|s| s.rec_str("block_number", &block.number));
        let Some(external_block) = block.external_block else {
            return log_and_err!("failed to mine external block because there is no external block being reexecuted");
        };

        // mine transactions
        let mut external_txs = Vec::with_capacity(block.transactions.len());
        for tx in block.transactions.into_values() {
            if let TransactionExecution::External(tx) = tx {
                external_txs.push(tx);
            } else {
                return log_and_err!("failed to mine external block because one of the transactions is not an external transaction");
            }
        }
        let mined_external_txs = mine_external_transactions(block.number, external_txs)?;

        block_from_external(external_block, mined_external_txs)
    }

    /// Same as [`Self::mine_local`], but automatically commits the block instead of returning it.
    /// mainly used when is_automine is enabled.
    pub fn mine_local_and_commit(&self) -> anyhow::Result<()> {
        let _mine_and_commit_lock = self.locks.mine_and_commit.lock().map_lock_error("mine_local_and_commit")?;

        let block = self.mine_local()?;
        self.commit(block)
    }

    /// Mines local transactions.
    ///
    /// External transactions are not allowed to be part of the block.
    pub fn mine_local(&self) -> anyhow::Result<Block> {
        #[cfg(feature = "tracing")]
        let _span = info_span!("miner::mine_local", block_number = field::Empty).entered();

        // lock
        let _mine_lock = self.locks.mine.lock().map_lock_error("mine_local")?;

        // mine block
        let block = self.storage.finish_pending_block()?;
        Span::with(|s| s.rec_str("block_number", &block.number));

        // mine transactions
        let mut local_txs = Vec::with_capacity(block.transactions.len());
        for tx in block.transactions.into_values() {
            if let TransactionExecution::Local(tx) = tx {
                local_txs.push(tx);
            } else {
                return log_and_err!("failed to mine local block because one of the transactions is not a local transaction");
            }
        }

        block_from_local(block.number, local_txs)
    }

    /// Persists a mined block to permanent storage and prepares new block.
    pub fn commit(&self, block: Block) -> anyhow::Result<()> {
        let block_number = block.number();

        // track
        #[cfg(feature = "tracing")]
        let _span = info_span!("miner::commit", %block_number).entered();
        tracing::info!(%block_number, transactions_len = %block.transactions.len(), "commiting block");

        // lock
        let _commit_lock = self.locks.commit.lock().map_lock_error("commit")?;

        tracing::info!(%block_number, "miner acquired commit lock");

        // extract fields to use in notifications if have subscribers
        let block_header = if self.notifier_blocks.receiver_count() > 0 {
            Some(block.header.clone())
        } else {
            None
        };
        let block_logs = if self.notifier_logs.receiver_count() > 0 {
            Some(block.transactions.iter().flat_map(|tx| &tx.logs).cloned().collect_vec())
        } else {
            None
        };

        // save storage
        self.storage.save_block(block)?;
        self.storage.set_mined_block_number(block_number)?;

        // notify
        if let Some(block_logs) = block_logs {
            for log in block_logs {
                let _ = self.notifier_logs.send(log);
            }
        }
        if let Some(block_header) = block_header {
            let _ = self.notifier_blocks.send(block_header);
        }

        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn mine_external_transactions(block_number: BlockNumber, txs: Vec<ExternalTransactionExecution>) -> anyhow::Result<Vec<TransactionMined>> {
    let mut mined_txs = Vec::with_capacity(txs.len());
    for tx in txs {
        if tx.tx.block_number()? != block_number {
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

pub fn block_from_local(number: BlockNumber, txs: Vec<LocalTransactionExecution>) -> anyhow::Result<Block> {
    // TODO: block timestamp should be set in the PendingBlock instead of being retrieved from the execution
    let block_timestamp = match txs.first() {
        Some(tx) => tx.result.execution.block_timestamp,
        None => UnixTime::now(),
    };

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
    use tokio_util::sync::CancellationToken;

    use crate::eth::miner::Miner;
    use crate::ext::MutexExt;
    use crate::infra::tracing::warn_task_cancellation;
    use crate::infra::tracing::warn_task_rx_closed;

    pub fn run(miner: Arc<Miner>, ticks_rx: mpsc::Receiver<Instant>, cancellation: CancellationToken) {
        const TASK_NAME: &str = "interval-miner-ticker";

        loop {
            if cancellation.is_cancelled() {
                warn_task_cancellation(TASK_NAME);
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

            if miner.is_paused() {
                tracing::warn!("skipping mining block because block mining is paused");
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
        let _mine_and_commit_lock = miner.locks.mine_and_commit.lock_or_clear("mutex in mine_and_commit is poisoned");

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
    use tokio::time::Instant;
    use tokio_util::sync::CancellationToken;

    use crate::infra::tracing::warn_task_cancellation;
    use crate::infra::tracing::warn_task_rx_closed;

    pub async fn run(block_time: Duration, ticks_tx: mpsc::Sender<Instant>, cancellation: CancellationToken) {
        const TASK_NAME: &str = "interval-miner-ticker";

        // sync to next second
        let next_second = (Utc::now() + Duration::from_secs(1))
            .with_nanosecond(0)
            .expect("nanosecond above is set to `0`, which is always less than 2 billion");

        let time_to_sleep = (next_second - Utc::now()).to_std().unwrap_or_default();
        thread::sleep(time_to_sleep);

        // prepare ticker
        let mut ticker = tokio::time::interval(block_time);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Burst);

        loop {
            if cancellation.is_cancelled() {
                warn_task_cancellation(TASK_NAME);
                return;
            }

            let tick = ticker.tick().await;
            if ticks_tx.send(tick).is_err() {
                warn_task_rx_closed(TASK_NAME);
                break;
            };
        }
    }
}
