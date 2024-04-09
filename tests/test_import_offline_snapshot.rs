use std::sync::Arc;
use std::time::Duration;

use fancy_duration::AsFancyDuration;
use itertools::Itertools;
use stratus::config::IntegrationTestConfig;
use stratus::eth::primitives::ExternalBlock;
use stratus::eth::primitives::ExternalReceipts;
use stratus::eth::primitives::StoragePointInTime;
use stratus::eth::storage::InMemoryPermanentStorageState;
use stratus::eth::storage::InMemoryTemporaryStorage;
use stratus::eth::storage::PermanentStorage;
use stratus::eth::storage::PostgresPermanentStorage;
use stratus::eth::storage::PostgresPermanentStorageConfig;
use stratus::eth::storage::StratusStorage;
use stratus::infra::docker::Docker;
use stratus::init_global_services;
#[cfg(feature = "metrics")]
mod m {
    pub use const_format::formatcp;
    pub use stratus::infra::metrics::METRIC_EVM_EXECUTION;
    pub use stratus::infra::metrics::METRIC_STORAGE_COMMIT;
    pub use stratus::infra::metrics::METRIC_STORAGE_READ_ACCOUNT;
    pub use stratus::infra::metrics::METRIC_STORAGE_READ_SLOT;
}

#[cfg(feature = "metrics")]
const METRIC_QUERIES: [&str; 43] = [
    // EVM
    "* EVM",
    m::formatcp!("{}_count", m::METRIC_EVM_EXECUTION),
    m::formatcp!("{}_sum", m::METRIC_EVM_EXECUTION),
    m::formatcp!("{}{{quantile='1'}}", m::METRIC_EVM_EXECUTION),
    m::formatcp!("{}{{quantile='0.95'}}", m::METRIC_EVM_EXECUTION),
    "* ACCOUNTS (count)",
    m::formatcp!("sum({}_count)", m::METRIC_STORAGE_READ_ACCOUNT),
    m::formatcp!("{}_count{{found_at='temporary'}}", m::METRIC_STORAGE_READ_ACCOUNT),
    m::formatcp!("{}_count{{found_at='permanent'}}", m::METRIC_STORAGE_READ_ACCOUNT),
    m::formatcp!("{}_count{{found_at='default'}}", m::METRIC_STORAGE_READ_ACCOUNT),
    "* ACCOUNTS (cumulative)",
    m::formatcp!("sum({}_sum)", m::METRIC_STORAGE_READ_ACCOUNT),
    m::formatcp!("{}_sum{{found_at='temporary'}}", m::METRIC_STORAGE_READ_ACCOUNT),
    m::formatcp!("{}_sum{{found_at='permanent'}}", m::METRIC_STORAGE_READ_ACCOUNT),
    m::formatcp!("{}_sum{{found_at='default'}}", m::METRIC_STORAGE_READ_ACCOUNT),
    "* ACCOUNTS (P100)",
    m::formatcp!("{}{{found_at='temporary', quantile='1'}}", m::METRIC_STORAGE_READ_ACCOUNT),
    m::formatcp!("{}{{found_at='permanent', quantile='1'}}", m::METRIC_STORAGE_READ_ACCOUNT),
    m::formatcp!("{}{{found_at='default', quantile='1'}}", m::METRIC_STORAGE_READ_ACCOUNT),
    "* ACCOUNTS (P95)",
    m::formatcp!("{}{{found_at='temporary', quantile='0.95'}}", m::METRIC_STORAGE_READ_ACCOUNT),
    m::formatcp!("{}{{found_at='permanent', quantile='0.95'}}", m::METRIC_STORAGE_READ_ACCOUNT),
    m::formatcp!("{}{{found_at='default', quantile='0.95'}}", m::METRIC_STORAGE_READ_ACCOUNT),
    "* STORAGE SLOTS (count)",
    m::formatcp!("sum({}_count)", m::METRIC_STORAGE_READ_SLOT),
    m::formatcp!("{}_count{{found_at='temporary'}}", m::METRIC_STORAGE_READ_SLOT),
    m::formatcp!("{}_count{{found_at='permanent'}}", m::METRIC_STORAGE_READ_SLOT),
    m::formatcp!("{}_count{{found_at='default'}}", m::METRIC_STORAGE_READ_SLOT),
    "* STORAGE SLOTS (cumulative)",
    m::formatcp!("sum({}_sum)", m::METRIC_STORAGE_READ_SLOT),
    m::formatcp!("{}_sum{{found_at='temporary'}}", m::METRIC_STORAGE_READ_SLOT),
    m::formatcp!("{}_sum{{found_at='permanent'}}", m::METRIC_STORAGE_READ_SLOT),
    m::formatcp!("{}_sum{{found_at='default'}}", m::METRIC_STORAGE_READ_SLOT),
    "* STORAGE SLOTS (P100)",
    m::formatcp!("{}{{found_at='temporary', quantile='1'}}", m::METRIC_STORAGE_READ_SLOT),
    m::formatcp!("{}{{found_at='permanent', quantile='1'}}", m::METRIC_STORAGE_READ_SLOT),
    m::formatcp!("{}{{found_at='default', quantile='1'}}", m::METRIC_STORAGE_READ_SLOT),
    "* STORAGE SLOTS (P95)",
    m::formatcp!("{}{{found_at='temporary', quantile='0.95'}}", m::METRIC_STORAGE_READ_SLOT),
    m::formatcp!("{}{{found_at='permanent', quantile='0.95'}}", m::METRIC_STORAGE_READ_SLOT),
    m::formatcp!("{}{{found_at='default', quantile='0.95'}}", m::METRIC_STORAGE_READ_SLOT),
    "* COMMIT",
    m::formatcp!("{}{{quantile='1'}}", m::METRIC_STORAGE_COMMIT),
];

#[cfg(not(feature = "metrics"))]
const METRIC_QUERIES: [&str; 0] = [];

#[tokio::test]
async fn test_import_offline_snapshot() {
    let mut config = init_global_services::<IntegrationTestConfig>();
    config.executor.chain_id = 2009;

    // init containers
    let docker = Docker::default();
    let _pg_guard = docker.start_postgres();
    let _prom_guard = docker.start_prometheus();

    // init block data
    let block_json = include_str!("fixtures/block-292973/block.json");
    let block: ExternalBlock = serde_json::from_str(block_json).unwrap();

    // init receipts data
    let receipts_json = include_str!("fixtures/block-292973/receipts.json");
    let receipts: ExternalReceipts = serde_json::from_str(receipts_json).unwrap();

    // init snapshot data
    let snapshot_json = include_str!("fixtures/block-292973/snapshot.json");
    let snapshot: InMemoryPermanentStorageState = serde_json::from_str(snapshot_json).unwrap();
    let pg = PostgresPermanentStorage::new(PostgresPermanentStorageConfig {
        url: docker.postgres_connection_url().to_owned(),
        connections: 5,
        acquire_timeout: Duration::from_secs(10),
    })
    .await
    .unwrap();
    populate_postgres(&pg, snapshot).await;

    // init executor and execute
    let storage = Arc::new(StratusStorage::new(Arc::new(InMemoryTemporaryStorage::default()), Arc::new(pg)));
    let executor = config.executor.init(storage);
    executor.import_external_to_perm(block, &receipts).await.unwrap();

    // get metrics from prometheus
    // sleep to ensure prometheus collected
    tokio::time::sleep(Duration::from_secs(5)).await;

    for query in METRIC_QUERIES {
        // formatting between query groups
        if query.starts_with('*') {
            println!("\n{}\n--------------------", query.replace("* ", ""));
            continue;
        }

        // get metrics and print them
        let url = format!("{}?query={}", docker.prometheus_api_url(), query);
        let response = reqwest::get(&url).await.unwrap().json::<serde_json::Value>().await.unwrap();
        let results = response.get("data").unwrap().get("result").unwrap().as_array().unwrap();
        if results.is_empty() {
            continue;
        }

        for result in results {
            let value: &str = result.get("value").unwrap().as_array().unwrap().last().unwrap().as_str().unwrap();
            let value: f64 = value.parse().unwrap();

            if query.contains("_count") {
                println!("{:<70} = {}", query, value);
            } else {
                let secs = Duration::from_secs_f64(value);
                println!("{:<70} = {}", query, secs.fancy_duration().truncate(2));
            }
        }
    }
}

async fn populate_postgres(pg: &PostgresPermanentStorage, state: InMemoryPermanentStorageState) {
    // save accounts
    let accounts = state.accounts.values().map(|a| a.to_account(&StoragePointInTime::Present)).collect_vec();
    pg.save_accounts(accounts).await.unwrap();

    // save slots
    let mut tx = pg.pool.begin().await.unwrap();
    for account in state.accounts.values() {
        for slot_history in account.slots.values() {
            let slot = slot_history.get_current();

            // we do not insert zero value slots because they were not really present in the storage when the snapshot was taken.
            if slot.is_zero() {
                continue;
            }

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
