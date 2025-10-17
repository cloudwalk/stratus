use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::bail;
use futures::try_join;
use tokio::sync::mpsc;

use crate::eth::executor::Executor;
use crate::eth::follower::consensus::Consensus;
use crate::eth::follower::importer::EXTERNAL_RPC_CURRENT_BLOCK;
use crate::eth::follower::importer::ImporterMode;
use crate::eth::follower::importer::LATEST_FETCHED_BLOCK_TIME;
use crate::eth::follower::importer::fetchers::DataFetcher;
use crate::eth::follower::importer::fetchers::block_with_changes::BlockWithChangesFetcher;
use crate::eth::follower::importer::fetchers::block_with_receipts::BlockWithReceiptsFetcher;
use crate::eth::follower::importer::importers::ImporterWorker;
use crate::eth::follower::importer::importers::execution::ReexecutionWorker;
use crate::eth::follower::importer::importers::fake_leader::FakeLeaderWorker;
use crate::eth::follower::importer::importers::replication::ReplicationWorker;
use crate::eth::follower::importer::start_number_fetcher;
use crate::eth::miner::Miner;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExecutionChanges;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::storage::StratusStorage;
use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;
use crate::ext::spawn;
use crate::infra::BlockchainClient;
use crate::infra::kafka::KafkaConnector;
use crate::utils::DropTimer;

type ReexecutionFollower =
    ImporterSupervisor<BlockWithReceiptsFetcher, ReexecutionWorker, (ExternalBlock, Vec<ExternalReceipt>), (ExternalBlock, Vec<ExternalReceipt>)>;
type FakeLeader = ImporterSupervisor<BlockWithReceiptsFetcher, FakeLeaderWorker, (ExternalBlock, Vec<ExternalReceipt>), (ExternalBlock, Vec<ExternalReceipt>)>;
type ReplicationFollower = ImporterSupervisor<BlockWithChangesFetcher, ReplicationWorker, (Block, BlockChangesRocksdb), (Block, ExecutionChanges)>;

pub struct ImporterSupervisor<Fetcher: DataFetcher<FT, PT>, Importer: ImporterWorker<PT>, FT: Send + 'static, PT: Send + 'static> {
    fetcher: Fetcher,
    importer: Importer,
    _phantom: std::marker::PhantomData<(FT, PT)>,
}

impl ReexecutionFollower {
    fn new(executor: Arc<Executor>, miner: Arc<Miner>, chain: Arc<BlockchainClient>, kafka_connector: Option<KafkaConnector>) -> Self {
        let importer = ReexecutionWorker {
            executor,
            miner,
            kafka_connector,
        };

        let fetcher = BlockWithReceiptsFetcher { chain: Arc::clone(&chain) };

        Self {
            fetcher,
            importer,
            _phantom: PhantomData,
        }
    }
}

impl FakeLeader {
    fn new(executor: Arc<Executor>, miner: Arc<Miner>, chain: Arc<BlockchainClient>) -> Self {
        let importer = FakeLeaderWorker { executor, miner };

        let fetcher = BlockWithReceiptsFetcher { chain: Arc::clone(&chain) };

        Self {
            fetcher,
            importer,
            _phantom: PhantomData,
        }
    }
}

impl ReplicationFollower {
    fn new(storage: Arc<StratusStorage>, miner: Arc<Miner>, chain: Arc<BlockchainClient>, kafka_connector: Option<KafkaConnector>) -> Self {
        let importer = ReplicationWorker { miner, kafka_connector };

        let fetcher = BlockWithChangesFetcher { chain, storage };

        Self {
            fetcher,
            importer,
            _phantom: PhantomData,
        }
    }
}

impl<Fetcher: DataFetcher<FT, PT> + 'static, Importer: ImporterWorker<PT> + 'static, FT: Send + 'static, PT: Send + 'static>
    ImporterSupervisor<Fetcher, Importer, FT, PT>
{
    async fn run(self, resume_from: BlockNumber, sync_interval: Duration, chain: Arc<BlockchainClient>) -> anyhow::Result<()> {
        let _timer = DropTimer::start("importer-online::run_importer_online");

        // Spawn common tasks: number fetcher
        let number_fetcher_task = spawn("importer::number-fetcher", start_number_fetcher(Arc::clone(&chain), sync_interval));

        let (backlog_tx, backlog_rx) = mpsc::channel(10_000);
        let importer_task = spawn("importer::importer", self.importer.run(backlog_rx));

        let fetcher_task = spawn("importer::fetcher", self.fetcher.run(backlog_tx, resume_from));

        let results = try_join!(importer_task, fetcher_task, number_fetcher_task)?;
        results.0?;
        results.1?;
        results.2?;

        Ok(())
    }
}

pub async fn start_importer(
    importer_mode: ImporterMode,
    storage: Arc<StratusStorage>,
    executor: Arc<Executor>,
    miner: Arc<Miner>,
    chain: Arc<BlockchainClient>,
    kafka_connector: Option<KafkaConnector>,
    sync_interval: Duration,
) -> anyhow::Result<()> {
    let resume_from: BlockNumber = storage.read_block_number_to_resume_import()?;

    match importer_mode {
        ImporterMode::BlockWithChanges => {
            ReplicationFollower::new(storage, miner, Arc::clone(&chain), kafka_connector)
                .run(resume_from, sync_interval, chain)
                .await?;
        }
        ImporterMode::ReexecutionFollower => {
            ReexecutionFollower::new(executor, miner, Arc::clone(&chain), kafka_connector)
                .run(resume_from, sync_interval, chain)
                .await?;
        }
        ImporterMode::FakeLeader =>
            FakeLeader::new(executor, miner, Arc::clone(&chain))
                .run(resume_from, sync_interval, chain)
                .await?,
    }
    Ok(())
}
pub struct ImporterConsensus {
    pub storage: Arc<StratusStorage>,
    pub chain: Arc<BlockchainClient>,
}

impl Consensus for ImporterConsensus {
    async fn lag(&self) -> anyhow::Result<u64> {
        let last_fetched_time = LATEST_FETCHED_BLOCK_TIME.load(Ordering::Relaxed);

        if last_fetched_time == 0 {
            bail!("stratus has not been able to connect to the leader yet");
        }

        let elapsed = chrono::Utc::now().timestamp() as u64 - last_fetched_time;
        if elapsed > 4 {
            Err(anyhow::anyhow!(
                "too much time elapsed without communicating with the leader. elapsed: {elapsed}s"
            ))
        } else {
            Ok(EXTERNAL_RPC_CURRENT_BLOCK.load(Ordering::SeqCst) - self.storage.read_mined_block_number().as_u64())
        }
    }

    fn get_chain(&self) -> anyhow::Result<&Arc<BlockchainClient>> {
        Ok(&self.chain)
    }
}
