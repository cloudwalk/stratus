use clap::Parser;
use csv::ReaderBuilder;
use std::path::PathBuf;

use anyhow::Result;
use ethereum_types::{Address, U256};
use stratus::eth::executor::{Executor, ExecutorConfig};
use stratus::eth::miner::Miner;
use stratus::eth::primitives::{BlockFilter, CallInput};
use stratus::eth::storage::{InMemoryTemporaryStorage, RocksPermanentStorage, StoragePointInTime, StratusStorage};
use std::sync::Arc;
use tokio::runtime::Runtime;
use std::fs::File;
use std::io::BufReader;
use indicatif::ProgressBar;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Opt {
    /// Path to the CSV file containing merchant addresses
    #[arg(short, long)]
    merchants_csv: PathBuf,

    /// Target block number for balance check
    #[arg(short, long)]
    block_number: u64,

    /// Contract address for the ERC20 token
    #[arg(short, long)]
    contract: Address,
}

fn main() -> Result<()> {
    let opt = Opt::parse();

    // Create a new tokio runtime
    let rt = Runtime::new()?;

    // Determine the block number to use
    let block_number = opt.block_number;

    // Read addresses from CSV
    let addresses = read_addresses_from_csv(&opt.merchants_csv)?;

    // Check balances
    let total_balance = rt.block_on(check_balances(
        addresses,
        opt.contract,
        block_number,
    ))?;

    println!("Total balance: {}", total_balance);

    Ok(())
}

fn read_addresses_from_csv(path: &PathBuf) -> Result<Vec<String>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut csv_reader = ReaderBuilder::new().has_headers(false).from_reader(reader);

    let mut addresses = Vec::new();
    for result in csv_reader.records() {
        let record = result?;
        if let Some(address_str) = record.get(0) {
            let address = address_str.trim_start_matches("0x").to_string();
            addresses.push(address);
        }
    }

    Ok(addresses)
}

async fn check_balances(
    addresses: Vec<String>,
    contract: Address,
    block_number: u64,
) -> Result<U256> {
    // Set up storage and executor (you might need to adjust this based on your project structure)
    let storage = Arc::new(StratusStorage::new(Box::<InMemoryTemporaryStorage>::default(), Box::new(RocksPermanentStorage::new(None)?)));
    let miner = Arc::new(Miner::new(Arc::clone(&storage), stratus::eth::miner::MinerMode::External, None));
    let executor = Arc::new(Executor::new(storage.clone(), miner, ExecutorConfig {
        chain_id: 2009,
        num_evms: 512,
        strategy: stratus::eth::executor::ExecutorStrategy::Paralell
    }));
    let point_in_time = storage.translate_to_point_in_time(&BlockFilter::Number(block_number.into()))?;

    use futures::stream::{self, StreamExt};

    let buffer_size = 100;
    let bar = ProgressBar::new(addresses.len() as u64);

    let balances_futures = stream::iter(addresses)
        .map(|address| {
            let executor = executor.clone();
            let bar = bar.clone();
            async move {
                tokio::task::spawn_blocking(move || {
                    get_balance(executor, address, contract, point_in_time, bar)
                })
                .await
            }
        })
        .buffer_unordered(buffer_size);

let mut total_balance = U256::zero();
    let mut balances_stream = balances_futures.fuse();
    while let Some(balance_result) = balances_stream.next().await {
        match balance_result {
            Ok(Ok(balance)) => {
                total_balance += balance;
            }
            _ => continue
        }
    }

    Ok(total_balance)
}

fn get_balance(
    executor: Arc<Executor>,
    address: String,
    contract: Address,
    point_in_time: StoragePointInTime,
    bar: ProgressBar
) -> Result<U256> {
    // Create the ERC20 balanceOf function call
    let data = hex::decode(format!("70a08231000000000000000000000000{}", address))?;

    let result = executor.execute_local_call(
        CallInput {
            from: None,
            to: Some(contract.into()),
            value: 0.into(),
            data: data.into(),
        },
        point_in_time,
    )?;

    if result.is_success() {
        let balance = U256::from(result.output.as_slice());
        bar.inc(1);
        Ok(balance)
    } else {
        bar.inc(1);
        Err(anyhow::anyhow!("Failed to get balance"))
    }
}
