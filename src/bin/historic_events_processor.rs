use std::borrow::Cow;
use std::time::Duration;

use anyhow::Context;
use chrono::Datelike;
use chrono::TimeZone;
use chrono::Timelike;
use indicatif::ProgressBar;
use rocksdb::properties::ESTIMATE_NUM_KEYS;
use stratus::eth::primitives::TransactionMined;
use stratus::eth::storage::permanent::rocks::types::BlockNumberRocksdb;
use stratus::eth::storage::permanent::rocks::types::BlockRocksdb;
use stratus::eth::storage::permanent::rocks::types::HashRocksdb;
use stratus::eth::storage::permanent::rocks::types::TransactionMinedRocksdb;
use stratus::eth::storage::permanent::rocks::types::UnixTimeRocksdb;
use stratus::eth::storage::permanent::rocks::RocksStorageState;
use stratus::ledger::events::transaction_to_events;
use stratus::ledger::events::AccountTransfers;
use stratus::ledger::events::Event;

/// Database timeout duration in seconds
const TIMEOUT: Duration = Duration::from_secs(5);

/// Converts a mined transaction from RocksDB to account transfer events
fn transaction_mined_rocks_db_to_events(
    block_timestamp: UnixTimeRocksdb,
    tx: TransactionMinedRocksdb,
    block_number: BlockNumberRocksdb,
    block_hash: HashRocksdb,
) -> Vec<AccountTransfers> {
    let tx = TransactionMined::from_rocks_primitives(tx, block_number, block_hash);
    transaction_to_events(block_timestamp.into(), Cow::Owned(tx))
}

/// Returns total count of blocks and transactions from RocksDB state
fn get_total_blocks_and_transactions(state: &RocksStorageState) -> (u64, u64) {
    let total_blocks = state
        .db
        .property_value_cf(&state.blocks_by_number.handle(), ESTIMATE_NUM_KEYS)
        .unwrap()
        .unwrap()
        .parse::<u64>()
        .unwrap();

    let total_transactions = state
        .db
        .property_value_cf(&state.transactions.handle(), ESTIMATE_NUM_KEYS)
        .unwrap()
        .unwrap()
        .parse::<u64>()
        .unwrap();

    (total_blocks, total_transactions)
}

/// Creates progress bars for tracking block and transaction processing
fn create_progress_bar(state: &RocksStorageState) -> (ProgressBar, ProgressBar) {
    tracing::info!("creating progress bar");
    let (total_blocks, total_transactions) = get_total_blocks_and_transactions(state);

    tracing::info!(?total_transactions, "Estimated total transactions in db:");
    let mb = indicatif::MultiProgress::new();
    let tx_pb = mb.add(indicatif::ProgressBar::new(total_transactions));
    let b_pb = mb.add(indicatif::ProgressBar::new(total_blocks));

    let style = indicatif::ProgressStyle::default_bar()
        .template("{msg}: [{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} ETA: {eta_precise}")
        .unwrap()
        .progress_chars("##-");

    tx_pb.set_style(style.clone());
    tx_pb.set_message("Transactions");
    b_pb.set_style(style);
    b_pb.set_message("Blocks");

    (b_pb, tx_pb)
}

/// Processes all transactions in a block and returns their event strings
fn process_block_events(block: BlockRocksdb) -> Vec<String> {
    let timestamp = block.header.timestamp;
    block
        .transactions
        .into_iter()
        .flat_map(|tx| transaction_mined_rocks_db_to_events(timestamp, tx, block.header.number, block.header.hash))
        .map(|event| event.event_payload().unwrap())
        .collect()
}

/// Main function that processes blockchain data and generates events
fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt::init();
    let state = RocksStorageState::new("data/rocksdb".to_string(), TIMEOUT, Some(0.1), false).context("failed to create rocksdb state")?;

    let (b_pb, tx_pb) = create_progress_bar(&state);

    // Load last processed block number from file
    tracing::info!("loading last processed block");
    let start_block = std::fs::read_to_string("last_processed_block")
        .map(|s| s.trim().parse::<u32>().unwrap())
        .unwrap_or(0);
    tracing::info!(?start_block);

    tracing::info!("creating rocksdb iterator");
    let iter = if start_block > 0 {
        b_pb.inc(start_block.into());
        state.blocks_by_number.iter_from(start_block.into(), rocksdb::Direction::Forward)?
    } else {
        state.blocks_by_number.iter_start()
    };

    let mut hours_since_0 = 0;
    let mut event_batch = vec![];
    let mut offset = std::fs::read_to_string("last_offset").map(|s| s.trim().parse::<usize>().unwrap()).unwrap_or(0);
    for result in iter {
        let (number, block) = result.context("failed to read block")?;
        let block = block.into_inner();

        let timestamp = block.header.timestamp;
        if hours_since_0 == 0 {
            hours_since_0 = timestamp.0 / 3600;
        }

        let tx_count = block.transactions.len();

        let mut block_events = process_block_events(block);

        event_batch.append(&mut block_events);

        tx_pb.inc(tx_count as u64);
        b_pb.inc(1);
        // Save current block number to file after processing
        if hours_since_0 != timestamp.0 / 3600 {
            let date = chrono::Utc.timestamp_opt((hours_since_0 * 3600) as i64, 0).earliest().unwrap();

            hours_since_0 = timestamp.0 / 3600;
            if !event_batch.is_empty() {
                let folder_path = format!(
                    "events/ledger_wallet_events/year={}/month={:02}/day={:02}/hour={:02}",
                    date.year(),
                    date.month(),
                    date.day(),
                    date.hour()
                );
                if !std::path::Path::new(&folder_path).exists() {
                    std::fs::create_dir_all(&folder_path)?;
                }
                std::fs::write(
                    format!("{}/ledger_wallet_events+backfill+{:010}.json", folder_path, offset),
                    event_batch.join("\n"),
                )?;
                offset += event_batch.len();
            }
            std::fs::write("last_processed_block", number.to_string())?;
            std::fs::write("last_offset", offset.to_string())?;
            event_batch.clear();
        }
    }

    tx_pb.finish_with_message("Done!");
    b_pb.finish_with_message("Done!");

    Ok(())
}
