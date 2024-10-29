use std::time::Duration;

use anyhow::Context;
use clap::CommandFactory;
use rocksdb::properties::ESTIMATE_NUM_KEYS;
use stratus::eth::primitives::TransactionMined;
use stratus::eth::storage::rocks::rocks_state::RocksStorageState;
use stratus::eth::storage::rocks::types::TransactionMinedRocksdb;
use stratus::eth::storage::rocks::types::UnixTimeRocksdb;
use stratus::infra::kafka::KafkaConfig;
use stratus::infra::kafka::KafkaSecurityProtocol;
use stratus::ledger::events::transaction_to_events;
use stratus::ledger::events::AccountTransfers;

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
    transaction_to_events(block_timestamp.into(), std::borrow::Cow::Owned(TransactionMined::from(tx)))
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

    let state = RocksStorageState::new("data/rocksdb".to_string(), TIMEOUT, None).context("failed to create rocksdb state")?;
    let connector = config.init()?;

    // Count total number of blocks first
    let total_transactions = state
        .db
        .property_value_cf(&state.transactions.handle(), ESTIMATE_NUM_KEYS)
        .unwrap()
        .unwrap()
        .parse::<u64>()
        .unwrap();

    let pb = indicatif::ProgressBar::new(total_transactions);

    pb.set_style(
        indicatif::ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg} ETA: {eta_precise}")
            .unwrap()
            .progress_chars("##-"),
    );

    for result in state.blocks_by_number.iter_start() {
        let (_number, block) = result.context("failed to read block")?;
        let timestamp = block.header.timestamp;
        for tx in block.into_inner().transactions {
            let events: Vec<AccountTransfers> = transaction_mined_rocks_db_to_events(timestamp, tx);

            for event in events {
                connector.send_event(event).await.context("failed to send event")?;
            }
            pb.inc(1);
        }
    }

    pb.finish_with_message("Done!");

    Ok(())
}
