use std::collections::HashMap;
use std::sync::Arc;

use itertools::Itertools;
use stratus::config::CommonConfig;
use stratus::eth::primitives::ExternalBlock;
use stratus::eth::primitives::ExternalReceipt;
use stratus::eth::primitives::StoragePointInTime;
use stratus::eth::storage::InMemoryPermanentStorageState;
use stratus::eth::storage::InMemoryTemporaryStorage;
use stratus::eth::storage::PermanentStorage;
use stratus::eth::storage::StratusStorage;
use stratus::infra::docker::Docker;
use stratus::infra::metrics::METRIC_EVM_EXECUTION;
use stratus::infra::metrics::METRIC_EVM_EXECUTION_ACCOUNT_READS;
use stratus::infra::metrics::METRIC_EVM_EXECUTION_SLOT_READS;
use stratus::infra::metrics::METRIC_EXECUTOR_IMPORT_OFFLINE_ACCOUNT_READS;
use stratus::infra::metrics::METRIC_EXECUTOR_IMPORT_OFFLINE_SLOT_READS;
use stratus::infra::metrics::METRIC_STORAGE_COMMIT;
use stratus::infra::metrics::METRIC_STORAGE_READ_ACCOUNT;
use stratus::infra::metrics::METRIC_STORAGE_READ_SLOT;
use stratus::infra::postgres::Postgres;
use stratus::init_global_services;

const METRIC_QUERIES: [&str; 8] = [
    METRIC_EVM_EXECUTION,
    METRIC_EVM_EXECUTION_SLOT_READS,
    METRIC_EVM_EXECUTION_ACCOUNT_READS,
    METRIC_EXECUTOR_IMPORT_OFFLINE_ACCOUNT_READS,
    METRIC_EXECUTOR_IMPORT_OFFLINE_SLOT_READS,
    METRIC_STORAGE_READ_ACCOUNT,
    METRIC_STORAGE_READ_SLOT,
    METRIC_STORAGE_COMMIT,
];

#[tokio::test]
async fn test_import_offline_snapshot() {
    let config = init_global_services::<CommonConfig>();

    // init containers
    let docker = Docker::default();
    let _pg_guard = docker.start_postgres();
    let _prom_guard = docker.start_prometheus();

    // init block data
    let block_json = include_str!("fixtures/block-292973/block.json");
    let block: ExternalBlock = serde_json::from_str(block_json).unwrap();

    // init receipts data
    let receipts_json = include_str!("fixtures/block-292973/receipts.json");
    let mut receipts = HashMap::new();
    for receipt_json in receipts_json.lines() {
        let receipt: ExternalReceipt = serde_json::from_str(receipt_json).unwrap();
        receipts.insert(receipt.hash(), receipt);
    }

    // init snapshot data
    let snapshot_json = include_str!("fixtures/block-292973/snapshot.json");
    let snapshot: InMemoryPermanentStorageState = serde_json::from_str(snapshot_json).unwrap();
    let pg = Postgres::new(docker.postgres_connection_url()).await.unwrap();
    populate_postgres(&pg, snapshot).await;

    // init executor and execute
    let storage = Arc::new(StratusStorage::new(Arc::new(InMemoryTemporaryStorage::default()), Arc::new(pg)));
    let executor = config.init_executor(storage);
    executor.import_offline(block, &receipts).await.unwrap();

    // get metrics from prometheus
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    for metric in METRIC_QUERIES {
        let url = format!("{}?query={}_sum", docker.prometheus_api_url(), metric);
        let response = reqwest::get(url).await.unwrap().json::<serde_json::Value>().await.unwrap();
        println!("\n{}\n{}", metric, response.get("data").unwrap().get("result").unwrap());
    }
}

async fn populate_postgres(pg: &Postgres, state: InMemoryPermanentStorageState) {
    // save accounts
    let accounts = state.accounts.values().map(|a| a.to_account(&StoragePointInTime::Present)).collect_vec();
    pg.save_accounts(accounts).await.unwrap();

    // save slots
    let mut tx = pg.start_transaction().await.unwrap();
    for account in state.accounts.values() {
        for slot_history in account.slots.values() {
            let slot = slot_history.get_current();
            sqlx::query("insert into account_slots(idx, value, account_address, creation_block) values($1, $2, $3, $4)")
                .bind(slot.index)
                .bind(slot.value)
                .bind(&account.address)
                .bind(0)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
    }
    tx.commit().await.unwrap();
}
