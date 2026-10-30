use std::sync::Arc;

use parking_lot::RwLock;
use stratus::GlobalServices;
use stratus::GlobalState;
use stratus::config::StratusConfig;
use stratus::eth::executor::Executor;
use stratus::eth::miner::Miner;
use stratus::eth::miner::MinerMode;
use stratus::eth::rpc::Server;
use stratus::eth::storage::StratusStorage;
use stratus::eth::storage::TransactionJournal;
use stratus::eth::types::BlockInfo;
#[cfg(all(not(target_env = "msvc"), any(feature = "jemalloc", feature = "jeprof")))]
use tikv_jemallocator::Jemalloc;

#[cfg(all(not(target_env = "msvc"), any(feature = "jemalloc", feature = "jeprof")))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

fn main() -> anyhow::Result<()> {
    let global_services = GlobalServices::<StratusConfig>::init();
    GlobalState::initialize_node_mode(&global_services.config);
    global_services.runtime.block_on(run(global_services.config))
}

/// Replays every journaled block left by a previous run, committing them to permanent storage.
fn recover_transaction_journal(executor: &Executor, miner: &Miner, storage: &StratusStorage) -> anyhow::Result<()> {
    let mined = storage.read_mined_block_number();
    for number in TransactionJournal::journaled_blocks()? {
        // every listed journal already exists, so the fresh timestamp is only used if a file vanished
        let mut journal = TransactionJournal::open(BlockInfo::new_at_now(number))?;
        if mined >= number {
            journal.retire()?;
            continue;
        }

        let Some(journal_block) = journal.block()? else {
            continue;
        };

        if journal_block.transactions.is_empty() && journal.is_live() {
            // An empty live journal belongs to the pending block being built, so there is nothing to recover.
            continue;
        }

        // Re-executing the transactions re-appends them to the journal file; removing the journal handle
        // from the pending state keeps the file as-is.
        let _pending_journal = storage.temp.transaction_storage.pending_block.write().journal.take();
        storage.set_pending_header(journal_block.header.number, *journal_block.header.timestamp);
        for transaction in journal_block.transactions {
            executor.execute_local_transaction(transaction, None)?;
        }

        miner.mine_local_and_commit()?;
        journal.retire()?;
    }
    Ok(())
}

async fn run(config: StratusConfig) -> anyhow::Result<()> {
    // Init services
    let storage = config.storage.init()?;

    // Init miner
    let miner = config.miner.init(Arc::clone(&storage));

    // Init executor
    let executor = config.executor.init(Arc::clone(&storage), Arc::clone(&miner));

    recover_transaction_journal(&executor, &miner, &storage)?;

    if let MinerMode::Interval(block_time) = miner.mode() {
        miner.start_interval_mining(block_time).await;
    }

    let (consensus, importer_runtime) = if let Some(importer_config) = &config.importer {
        tracing::info!(?importer_config, "creating importer");
        let kafka_connector = config.kafka_config.as_ref().map(|inner| inner.init()).transpose()?;
        match importer_config
            .init(Arc::clone(&executor), Arc::clone(&miner), Arc::clone(&storage), kafka_connector)
            .await?
        {
            Some((consensus, importer_runtime)) => (Some(consensus), Some(importer_runtime)),
            None => (None, None),
        }
    } else {
        tracing::info!("no importer config, skipping importer");
        (None, None)
    };
    let consensus = Arc::new(RwLock::new(consensus));
    let importer_runtime = Arc::new(RwLock::new(importer_runtime));

    // Init exporter runtime
    let exporter_runtime = config.exporter.init().await?;
    let exporter_runtime = Arc::new(RwLock::new(Some(exporter_runtime)));

    // Init RPC server
    Server::new(
        // Services
        Arc::clone(&storage),
        executor,
        miner,
        consensus,
        importer_runtime,
        exporter_runtime,
        // Config
        config.clone(),
        config.rpc_server,
        config.executor.executor_chain_id.into(),
    )
    .serve()
    .await?;

    // Explicitly block the `main` thread to drop the storage.
    drop(storage);

    Ok(())
}
