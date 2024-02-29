#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use itertools::Itertools;
use serde_json::Value as JsonValue;
use sqlx::Row;
use stratus::config::ImporterImportConfig;
use stratus::eth::primitives::Account;
use stratus::eth::primitives::Address;
use stratus::eth::primitives::BlockNumber;
use stratus::eth::primitives::ExternalBlock;
use stratus::eth::primitives::ExternalReceipt;
use stratus::eth::primitives::Wei;
use stratus::eth::storage::StratusStorage;
use stratus::infra::postgres::Postgres;
use stratus::infra::BlockchainClient;
use stratus::init_global_services;
use stratus::log_and_err;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Number of tasks in backlog: (BACKLOG_SIZE * BacklogTask)
const BACKLOG_SIZE: usize = 10;
const RPC_TIMEOUT: Duration = Duration::from_secs(2);
type BacklogTask = (Vec<BlockRow>, Vec<ReceiptRow>);

#[tokio::main()]
async fn main() -> anyhow::Result<()> {
    // init services
    let config: ImporterImportConfig = init_global_services();
    let pg = Arc::new(Postgres::new(&config.postgres_url).await?);
    let storage = config.init_storage().await?;
    let executor = config.init_executor(Arc::clone(&storage));

    // init shared data between importer and postgres loader
    let (backlog_tx, mut backlog_rx) = mpsc::channel::<BacklogTask>(BACKLOG_SIZE);
    let cancellation = CancellationToken::new();

    // import genesis accounts
    let balances = db_retrieve_balances(&pg).await?;
    let accounts = balances
        .into_iter()
        .map(|row| Account::new_with_balance(row.address, row.balance))
        .collect_vec();
    storage.save_accounts_to_perm(accounts).await?;

    let chain = BlockchainClient::new(&config.external_rpc, RPC_TIMEOUT)?;
    let mut latest_compared_block = storage.read_current_block_number().await?;
    loop {
        let current_imported_block = storage.read_current_block_number().await?;
        if current_imported_block - latest_compared_block >= interval {
            let result = validate_state(&chain, Arc::clone(&storage), latest_compared_block, latest_compared_block + interval, max_sample_size)
                .await;
            if let Err(err) = result {
                cancellation.cancel();
                panic!("{}", err);
            }
            latest_compared_block = current_imported_block;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Ok(())
}

async fn keep_validating_state(storage: Arc<StratusStorage>, external_rpc: String, max_sample_size: u64, interval: BlockNumber, cancellation: CancellationToken) -> anyhow::Result<()> {
    println!("STARTING STATE VALIDATION");

}

async fn validate_state(
    chain: &BlockchainClient,
    storage: Arc<StratusStorage>,
    start: BlockNumber,
    end: BlockNumber,
    max_sample_size: u64,
) -> anyhow::Result<()> {
    println!("Validating state {:?}, {:?}", start, end);
    let slots = storage.get_slots_sample(start, end, max_sample_size, Some(1)).await?;
    for sampled_slot in slots {
        let expected_value = chain
            .get_storage_at(
                &sampled_slot.address,
                &sampled_slot.index,
                stratus::eth::primitives::StoragePointInTime::Past(sampled_slot.block_number),
            )
            .await?;
        println!("Comparing {:?} {:?}", sampled_slot.value, expected_value);
        if sampled_slot.value != expected_value {
            return Err(anyhow::anyhow!(
                "State mismatch on slot {:?}, expected value: {:?}, found: {:?}",
                sampled_slot,
                expected_value,
                sampled_slot.value
            ));
        }
    }
    Ok(())
}
