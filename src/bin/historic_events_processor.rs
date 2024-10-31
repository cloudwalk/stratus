use std::time::Duration;

use anyhow::Context;
use clap::CommandFactory;
use futures::StreamExt;
use rocksdb::properties::ESTIMATE_NUM_KEYS;
use stratus::eth::storage::rocks::rocks_state::RocksStorageState;
use stratus::eth::storage::rocks::types::TransactionMinedRocksdb;
use stratus::eth::storage::rocks::types::UnixTimeRocksdb;
use stratus::infra::kafka::KafkaConfig;
use stratus::infra::kafka::KafkaSecurityProtocol;
use stratus::ledger::events::transaction_to_events;
use stratus::ledger::events::AccountTransfers;
use stratus::log_and_err;

const TIMEOUT: Duration = Duration::from_secs(5);

fn parse_config(args: &clap::ArgMatches) -> KafkaConfig {
    KafkaConfig {
        bootstrap_servers: args.get_one::<String>("bootstrap_servers").unwrap().clone(),
        topic: args.get_one::<String>("topic").unwrap().clone(),
        client_id: args.get_one::<String>("client_id").unwrap().clone(),
        group_id: args.get_one::<String>("group_id").cloned(),
        security_protocol: args.get_one::<KafkaSecurityProtocol>("security_protocol").copied().unwrap_or_default(),
        sasl_mechanisms: args.get_one::<String>("sasl_mechanisms").cloned(),
        sasl_username: args.get_one::<String>("sasl_username").cloned(),
        sasl_password: args.get_one::<String>("sasl_password").cloned(),
        ssl_ca_location: args.get_one::<String>("ssl_ca_location").cloned(),
        ssl_certificate_location: args.get_one::<String>("ssl_certificate_location").cloned(),
        ssl_key_location: args.get_one::<String>("ssl_key_location").cloned(),
    }
}

fn transaction_mined_rocks_db_to_events(block_timestamp: UnixTimeRocksdb, tx: TransactionMinedRocksdb) -> Vec<AccountTransfers> {
    transaction_to_events(block_timestamp.into(), std::borrow::Cow::Owned(tx.into()))
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt::init();

    // trick clap to ignore the ImporterConfig requirement
    let config = parse_config(
        &KafkaConfig::command()
            .mut_group("KafkaConfig", |group| group.id("ImporterConfig"))
            .get_matches(),
    );

    let state = RocksStorageState::new("data/rocksdb".to_string(), TIMEOUT, Some(0.1)).context("failed to create rocksdb state")?;
    let connector = config.init()?;

    // Count total number of blocks first
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

    // Load last processed block number from file
    let start_block = std::fs::read_to_string("last_processed_block")
        .map(|s| s.trim().parse::<u64>().unwrap())
        .unwrap_or(0);

    let iter = if start_block > 0 {
        b_pb.inc(start_block);
        state.blocks_by_number.iter_from(start_block.into(), rocksdb::Direction::Forward)?
    } else {
        state.blocks_by_number.iter_start()
    };

    for result in iter {
        let (number, block) = result.context("failed to read block")?;
        let block = block.into_inner();

        let timestamp = block.header.timestamp;
        let tx_count = block.transactions.len();

        let block_events = block
            .transactions
            .into_iter()
            .flat_map(|tx| transaction_mined_rocks_db_to_events(timestamp, tx));

        let mut buffer = connector.send_buffered(block_events.collect(), 30)?;
        while let Some(res) = buffer.next().await {
            if let Err(e) = res {
                return log_and_err!(reason = e, "failed to send events");
            }
        }
        tx_pb.inc(tx_count as u64);
        b_pb.inc(1);
        // Save current block number to file after processing
        std::fs::write("last_processed_block", number.to_string())?;
    }

    tx_pb.finish_with_message("Done!");
    b_pb.finish_with_message("Done!");

    Ok(())
}
