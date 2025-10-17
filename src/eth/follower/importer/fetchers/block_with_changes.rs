use std::cmp::min;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use alloy_rpc_types_eth::BlockTransactions;
use anyhow::anyhow;
use anyhow::bail;
use async_trait::async_trait;
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio::task::yield_now;

use crate::eth::follower::importer::EXTERNAL_RPC_CURRENT_BLOCK;
use crate::eth::follower::importer::create_execution_changes;
use crate::eth::follower::importer::fetch_with_retry;
use crate::eth::follower::importer::fetchers::FetcherWorker;
use crate::eth::follower::importer::should_shutdown;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExecutionChanges;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::storage::StratusStorage;
use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;
use crate::globals::IMPORTER_ONLINE_TASKS_SEMAPHORE;
use crate::infra::BlockchainClient;
use crate::infra::tracing::warn_task_rx_closed;

pub struct BlockChangesFetcherWorker {
    pub chain: Arc<BlockchainClient>,
    pub storage: Arc<StratusStorage>,
}

#[async_trait]
impl FetcherWorker<(Block, BlockChangesRocksdb), (Block, ExecutionChanges)> for BlockChangesFetcherWorker {
    async fn fetch(&self, block_number: BlockNumber) -> (Block, BlockChangesRocksdb) {
        let fetch_fn = |bn| self.chain.fetch_block_with_changes(bn);
        fetch_with_retry(block_number, fetch_fn, "block and changes").await
    }

    async fn post_process(&self, data: (Block, BlockChangesRocksdb)) -> anyhow::Result<(Block, ExecutionChanges)> {
        let storage = Arc::clone(&self.storage);
        let (block, changes) = data;
        let changes = create_execution_changes(&storage, changes)?;
        Ok((block, changes))
    }
}
