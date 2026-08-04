use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::anyhow;
use parking_lot::Mutex;
use parking_lot::RwLock;
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::broadcast;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::Span;

use crate::eth::miner::MinerMode;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockHeader;
use crate::eth::primitives::ExecutionChanges;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogMessage;
use crate::eth::primitives::StorageError;
use crate::eth::primitives::StratusError;
use crate::eth::primitives::TransactionExecution;
use crate::eth::storage::BlockReference;
use crate::eth::storage::PendingBlockGuard;
use crate::eth::storage::StratusStorage;
use crate::ext::DisplayExt;
use crate::ext::not;
use crate::globals::STRATUS_SHUTDOWN_SIGNAL;
use crate::infra::tracing::SpanExt;

cfg_if::cfg_if! {
    if #[cfg(feature = "tracing")] {
        use tracing::field;
        use tracing::info_span;
    }
}

/// Represents different types of items that can be committed to storage
#[allow(clippy::large_enum_variant)]
pub enum CommitItem {
    /// A block
    Block(Block),
    /// A block that wasn't executed in this node and instead contains all changes already pre-computed
    ReplicationBlock(Block),
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
    pub notifier_logs: broadcast::Sender<LogMessage>,

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
    pub mine_and_commit: Mutex<()>,
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

    pub fn pending_block_guard(&self) -> PendingBlockGuard<'_> {
        self.storage.pending_block_guard()
    }

    #[cfg(feature = "dev")]
    pub fn reset_to_genesis(&self) -> Result<(), StorageError> {
        let _mine_and_commit_guard = self.locks.mine_and_commit.lock();
        let pending_guard = self.pending_block_guard();
        let _commit_guard = self.locks.commit.lock();
        self.storage.reset_to_genesis(&pending_guard)
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

        *self.shutdown_signal.lock() = new_shutdown_signal;
        *self.interval_joinset.lock().await = Some(joinset);
    }

    /// Shuts down interval miner, set miner mode to External.
    pub async fn switch_to_external_mode(self: &Arc<Self>) {
        if self.mode().is_external() {
            tracing::warn!("trying to change mode to external, but it's already set, skipping");
            return;
        }
        self.shutdown_and_wait().await;
        self.set_mode(MinerMode::External);
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
        *self.mode.read()
    }

    fn set_mode(&self, new_mode: MinerMode) {
        *self.mode.write() = new_mode;
    }

    pub fn is_interval_miner_running(&self) -> bool {
        match self.interval_joinset.try_lock() {
            // check if the joinset of tasks has futures running
            Ok(joinset) => joinset.as_ref().is_some_and(|joinset| not(joinset.is_empty())),
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

        tracing::warn!("shutting down interval miner to switch to external mode");

        self.shutdown_signal.lock().cancel();

        // wait for all tasks to end
        while let Some(result) = joinset.join_next().await {
            if let Err(e) = result {
                tracing::error!(reason = ?e, "miner task failed");
            }
        }
    }

    /// Persists a transaction execution.
    pub fn save_execution(&self, tx_execution: TransactionExecution) -> Result<(), StratusError> {
        // Check if automine is enabled
        let is_automine = self.mode().is_automine();

        if is_automine {
            let _mine_and_commit_lock = self.locks.mine_and_commit.lock();
            let pending_guard = self.pending_block_guard();
            self.save_execution_with_guard(&pending_guard, tx_execution)?;
            let (block, changes) = self.mine_local_with_guard(&pending_guard)?;
            drop(pending_guard);
            self.commit(CommitItem::Block(block), changes)?;
        } else {
            let pending_guard = self.pending_block_guard();
            self.save_execution_with_guard(&pending_guard, tx_execution)?;
        }

        Ok(())
    }

    pub(crate) fn save_execution_with_guard(&self, guard: &PendingBlockGuard<'_>, tx_execution: TransactionExecution) -> Result<(), StratusError> {
        let tx_hash = tx_execution.info.hash;

        #[cfg(feature = "tracing")]
        let _span = info_span!("miner::save_execution", %tx_hash).entered();

        self.storage.save_execution(guard, tx_execution)?;

        if self.has_pending_tx_subscribers() {
            self.send_pending_tx_notification(&Some(tx_hash));
        }

        Ok(())
    }

    /// Mines an external block inside the same pending-state session that executed its transactions.
    pub fn mine_external_with_guard(&self, external_block: ExternalBlock, guard: &PendingBlockGuard<'_>) -> anyhow::Result<(Block, ExecutionChanges)> {
        #[cfg(feature = "tracing")]
        let _span = info_span!("miner::mine_external", block_number = field::Empty).entered();

        let parent_hash = self.storage.read_pending_parent_hash(guard);
        let (pending_block, changes) = self.storage.pending_block_to_seal(guard);
        let timestamp = pending_block.header.timestamp.clone();
        let mut block = Block::from_pending(pending_block, parent_hash);

        Span::with(|s| s.rec_str("block_number", &block.header.number));
        let external_parent_hash = external_block.parent_hash();
        block.apply_external(&external_block)?;
        // Preserve the imported parent so save_block can validate continuity against last_saved.
        // V2 already commits to this field; the assignment is relevant to the temporary V1 fallback.
        block.header.parent_hash = external_parent_hash;

        match external_block == block {
            true => {
                self.storage.finish_pending_block(guard, BlockReference::from(&block), timestamp);
                self.storage.publish_block_hash(block.number(), block.hash());
                Ok((block, changes))
            }
            false => Err(anyhow!(
                "mismatching block info:\n\tlocal:\n\t\tnumber: {:?}\n\t\ttimestamp: {:?}\n\t\thash: {:?}\n\texternal:\n\t\tnumber: {:?}\n\t\ttimestamp: {:?}\n\t\thash: {:?}",
                block.number(),
                block.header.timestamp,
                block.hash(),
                external_block.number(),
                external_block.timestamp(),
                external_block.hash()
            )),
        }
    }

    /// Same as [`Self::mine_local`], but automatically commits the block instead of returning it.
    /// mainly used when is_automine is enabled.
    pub fn mine_local_and_commit(&self) -> anyhow::Result<(), StorageError> {
        let _mine_and_commit_lock = self.locks.mine_and_commit.lock();
        let pending_guard = self.pending_block_guard();
        let (block, changes) = self.mine_local_with_guard(&pending_guard)?;
        drop(pending_guard);
        self.commit(CommitItem::Block(block), changes)
    }

    /// Mines local transactions.
    ///
    /// External transactions are not allowed to be part of the block.
    pub fn mine_local(&self) -> anyhow::Result<(Block, ExecutionChanges), StorageError> {
        #[cfg(feature = "tracing")]
        let _span = info_span!("miner::mine_local", block_number = field::Empty).entered();

        let pending_guard = self.pending_block_guard();
        self.mine_local_with_guard(&pending_guard)
    }

    pub(crate) fn mine_local_with_guard(&self, guard: &PendingBlockGuard<'_>) -> anyhow::Result<(Block, ExecutionChanges), StorageError> {
        let parent_hash = self.storage.read_pending_parent_hash(guard);
        let (pending_block, changes) = self.storage.pending_block_to_seal(guard);
        let timestamp = pending_block.header.timestamp.clone();
        let block = Block::from_pending(pending_block, parent_hash);
        self.storage.finish_pending_block(guard, BlockReference::from(&block), timestamp);
        self.storage.publish_block_hash(block.number(), block.hash());
        Span::with(|s| s.rec_str("block_number", &block.header.number));

        Ok((block, changes))
    }

    pub(crate) fn validate_next_saved_block(&self, block: &Block) -> Result<(), StorageError> {
        self.storage.validate_next_saved_block(block)
    }

    pub fn commit(&self, item: CommitItem, changes: ExecutionChanges) -> anyhow::Result<(), StorageError> {
        match item {
            CommitItem::Block(block) => self.commit_block(block, changes),
            CommitItem::ReplicationBlock(block) => {
                let pending_guard = self.pending_block_guard();
                self.storage.set_pending_header(&pending_guard, block.number(), block.timestamp());
                self.storage
                    .finish_pending_block(&pending_guard, BlockReference::from(&block), block.timestamp().into());
                self.storage.publish_block_hash(block.number(), block.hash());
                drop(pending_guard);
                self.commit_block(block, changes)
            }
        }
    }

    /// Persists a mined block to permanent storage and prepares new block.
    pub fn commit_block(&self, block: Block, changes: ExecutionChanges) -> anyhow::Result<(), StorageError> {
        let block_number = block.number();

        // track
        #[cfg(feature = "tracing")]
        let _span = info_span!("miner::commit", %block_number).entered();
        tracing::info!(%block_number, transactions_len = %block.transactions.len(), "commiting block");

        // lock
        let _commit_lock = self.locks.commit.lock();

        tracing::info!(%block_number, "miner acquired commit lock");

        // extract fields to use in notifications if have subscribers
        let block_header = if self.has_block_header_subscribers() {
            Some(block.header.clone())
        } else {
            None
        };
        let block_logs = self.has_log_subscribers().then(|| block.create_log_messages());

        // save storage
        self.storage.save_block(block, changes)?;

        // Send notifications after saving the block
        self.send_log_notifications(&block_logs);
        self.send_block_header_notification(&block_header);

        Ok(())
    }

    // -----------------------------------------------------------------------------
    // Notification methods
    // -----------------------------------------------------------------------------

    /// Checks if there are any subscribers for block header notifications
    fn has_block_header_subscribers(&self) -> bool {
        self.notifier_blocks.receiver_count() > 0
    }

    /// Checks if there are any subscribers for log notifications
    fn has_log_subscribers(&self) -> bool {
        self.notifier_logs.receiver_count() > 0
    }

    /// Checks if there are any subscribers for pending transaction notifications
    fn has_pending_tx_subscribers(&self) -> bool {
        self.notifier_pending_txs.receiver_count() > 0
    }

    /// Sends a notification for a block header
    fn send_block_header_notification(&self, block_header: &Option<BlockHeader>) {
        if let Some(block_header) = block_header {
            let _ = self.notifier_blocks.send(block_header.clone());
        }
    }

    /// Sends notifications for logs
    fn send_log_notifications(&self, logs: &Option<Vec<LogMessage>>) {
        if let Some(logs) = logs {
            for log in logs {
                let _ = self.notifier_logs.send(log.clone());
            }
        }
    }

    /// Sends notifications for pending transactions
    fn send_pending_tx_notification(&self, tx_hash: &Option<Hash>) {
        if let Some(tx_hash) = tx_hash {
            let _ = self.notifier_pending_txs.send(*tx_hash);
        }
    }
}

// -----------------------------------------------------------------------------
// Miner
// -----------------------------------------------------------------------------
pub mod interval_miner {
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Duration;

    use parking_lot::MutexGuard;
    use tokio::time::Instant;
    use tokio_util::sync::CancellationToken;

    use crate::eth::miner::Miner;
    use crate::eth::miner::miner::CommitItem;
    use crate::eth::primitives::Block;
    use crate::eth::primitives::ExecutionChanges;
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
            let (block, changes, miner_guard) = mine_local_retry(&miner);
            commit_retry(&miner, block, changes, miner_guard);
        }
        warn_task_rx_closed(TASK_NAME);
    }

    pub fn mine_local_retry(miner: &Miner) -> (Block, ExecutionChanges, MutexGuard<'_, ()>) {
        let guard = miner.locks.mine_and_commit.lock();
        loop {
            match miner.mine_local() {
                Ok((block, changes)) => break (block, changes, guard),
                Err(e) => {
                    tracing::error!(reason = ?e, "failed to mine block");
                }
            }
        }
    }

    pub fn commit_retry(miner: &Miner, block: Block, changes: ExecutionChanges, _miner_guard: MutexGuard<()>) {
        loop {
            match miner.commit(CommitItem::Block(block.clone()), changes.clone()) {
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
        #[allow(clippy::expect_used)]
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

#[cfg(test)]
mod tests {
    use fake::Fake;
    use fake::Faker;

    use super::*;
    use crate::eth::primitives::BlockNumber;

    fn initialize_genesis(storage: &Arc<StratusStorage>) -> Block {
        if let Some(genesis) = storage.read_block(crate::eth::primitives::BlockFilter::Number(BlockNumber::ZERO)).unwrap() {
            return genesis;
        }

        let genesis = Block::genesis();
        storage
            .save_genesis_block(genesis.clone(), Vec::new(), ExecutionChanges::default())
            .expect("save genesis block");
        genesis
    }

    #[test]
    fn local_mining_uses_latest_sealed_hash_when_cache_is_empty() {
        let storage = Arc::new(StratusStorage::new_test().expect("create test storage"));
        let miner = Miner::new(Arc::clone(&storage), MinerMode::Automine);
        let genesis = initialize_genesis(&storage);
        storage.clear_cache();

        let (block, _) = miner.mine_local().expect("mine local block");

        assert_eq!(block.number(), BlockNumber::ONE);
        assert_eq!(block.header.parent_hash, genesis.hash());
    }

    #[test]
    fn invalid_external_hash_does_not_advance_pending_block() {
        let storage = Arc::new(StratusStorage::new_test().expect("create test storage"));
        let miner = Miner::new(Arc::clone(&storage), MinerMode::External);
        initialize_genesis(&storage);

        let mut external_block: ExternalBlock = Faker.fake();
        external_block.0.header.inner.number = 1;
        external_block.0.header.hash = alloy_primitives::B256::ZERO;

        let pending_guard = miner.pending_block_guard();
        storage.set_pending_from_external(&pending_guard, &external_block);
        miner
            .mine_external_with_guard(external_block, &pending_guard)
            .expect_err("invalid external hash should be rejected");

        assert_eq!(storage.read_pending_block_header().0.number, BlockNumber::ONE);
    }

    #[test]
    fn legacy_external_parent_is_validated_when_saved() {
        let storage = Arc::new(StratusStorage::new_test().expect("create test storage"));
        let miner = Miner::new(Arc::clone(&storage), MinerMode::External);
        let genesis = initialize_genesis(&storage);

        let mut external_block: ExternalBlock = Faker.fake();
        external_block.0.header.inner.number = 1;
        external_block.0.header.inner.parent_hash = alloy_primitives::B256::ZERO;
        external_block.0.header.hash = BlockNumber::ONE.hash().into();
        external_block.0.transactions = alloy_rpc_types_eth::BlockTransactions::Full(Vec::new());

        let pending_guard = miner.pending_block_guard();
        storage.set_pending_from_external(&pending_guard, &external_block);
        let (block, changes) = miner
            .mine_external_with_guard(external_block, &pending_guard)
            .expect("legacy hash should be accepted while importing");
        drop(pending_guard);

        assert!(matches!(
            miner.validate_next_saved_block(&block),
            Err(StorageError::ParentHashConflict { number, local, external })
                if number == BlockNumber::ONE && local == genesis.hash() && external == Hash::ZERO
        ));
        let error = storage.save_block(block, changes).expect_err("disconnected external parent should be rejected");
        assert!(matches!(
            error,
            StorageError::ParentHashConflict { number, local, external }
                if number == BlockNumber::ONE && local == genesis.hash() && external == Hash::ZERO
        ));
    }

    #[test]
    fn pending_block_guard_serializes_pending_writers() {
        let storage = Arc::new(StratusStorage::new_test().expect("create test storage"));
        let miner = Arc::new(Miner::new(storage, MinerMode::External));
        let first_guard = miner.pending_block_guard();
        let (acquired_tx, acquired_rx) = std::sync::mpsc::channel();

        let other_miner = Arc::clone(&miner);
        let handle = std::thread::spawn(move || {
            let _guard = other_miner.pending_block_guard();
            acquired_tx.send(()).expect("notify guard acquisition");
        });

        assert!(acquired_rx.recv_timeout(Duration::from_millis(20)).is_err());
        drop(first_guard);
        acquired_rx.recv_timeout(Duration::from_secs(1)).expect("second writer should acquire guard");
        handle.join().expect("join guard thread");
    }
}
