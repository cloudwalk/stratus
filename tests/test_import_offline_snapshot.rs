use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use const_format::formatcp;
use fancy_duration::AsFancyDuration;
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
use stratus::infra::metrics::METRIC_STORAGE_COMMIT;
use stratus::infra::metrics::METRIC_STORAGE_READ_ACCOUNT;
use stratus::infra::metrics::METRIC_STORAGE_READ_SLOT;
use stratus::infra::postgres::Postgres;
use stratus::init_global_services;

const METRIC_QUERIES: [&str; 30] = [
    // EVM
    "",
    formatcp!("{}_count", METRIC_EVM_EXECUTION),
    formatcp!("{}_sum", METRIC_EVM_EXECUTION),
    formatcp!("{}{{quantile='1'}}", METRIC_EVM_EXECUTION),
    // STORAGE ACCOUNTS
    "",
    formatcp!("sum({}_count)", METRIC_STORAGE_READ_ACCOUNT),
    formatcp!("{}_count{{found_at='temporary'}}", METRIC_STORAGE_READ_ACCOUNT),
    formatcp!("{}_count{{found_at='permanent'}}", METRIC_STORAGE_READ_ACCOUNT),
    formatcp!("{}_count{{found_at='default'}}", METRIC_STORAGE_READ_ACCOUNT),
    formatcp!("sum({}_sum)", METRIC_STORAGE_READ_ACCOUNT),
    formatcp!("{}_sum{{found_at='temporary'}}", METRIC_STORAGE_READ_ACCOUNT),
    formatcp!("{}_sum{{found_at='permanent'}}", METRIC_STORAGE_READ_ACCOUNT),
    formatcp!("{}_sum{{found_at='default'}}", METRIC_STORAGE_READ_ACCOUNT),
    formatcp!("{}{{found_at='temporary', quantile='1'}}", METRIC_STORAGE_READ_ACCOUNT),
    formatcp!("{}{{found_at='permanent', quantile='1'}}", METRIC_STORAGE_READ_ACCOUNT),
    formatcp!("{}{{found_at='default', quantile='1'}}", METRIC_STORAGE_READ_ACCOUNT),
    // STORAGE SLOTS
    "",
    formatcp!("sum({}_count)", METRIC_STORAGE_READ_SLOT),
    formatcp!("{}_count{{found_at='temporary'}}", METRIC_STORAGE_READ_SLOT),
    formatcp!("{}_count{{found_at='permanent'}}", METRIC_STORAGE_READ_SLOT),
    formatcp!("{}_count{{found_at='default'}}", METRIC_STORAGE_READ_SLOT),
    formatcp!("sum({}_sum)", METRIC_STORAGE_READ_SLOT),
    formatcp!("{}_sum{{found_at='temporary'}}", METRIC_STORAGE_READ_SLOT),
    formatcp!("{}_sum{{found_at='permanent'}}", METRIC_STORAGE_READ_SLOT),
    formatcp!("{}_sum{{found_at='default'}}", METRIC_STORAGE_READ_SLOT),
    formatcp!("{}{{found_at='temporary', quantile='1'}}", METRIC_STORAGE_READ_SLOT),
    formatcp!("{}{{found_at='permanent', quantile='1'}}", METRIC_STORAGE_READ_SLOT),
    formatcp!("{}{{found_at='default', quantile='1'}}", METRIC_STORAGE_READ_SLOT),
    // STORAGE COMMIT
    "",
    formatcp!("{}{{quantile='1'}}", METRIC_STORAGE_COMMIT),
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
    let mut receipts = receipts_json
        .lines()
        .map(|json| serde_json::from_str::<ExternalReceipt>(json).unwrap())
        .collect_vec()
        .into();

    // init snapshot data
    let snapshot_json = include_str!("fixtures/block-292973/snapshot.json");
    let snapshot: InMemoryPermanentStorageState = serde_json::from_str(snapshot_json).unwrap();
    let pg = Postgres::new(docker.postgres_connection_url(), 10usize, 2usize).await.unwrap();
    populate_postgres(&pg, snapshot).await;

    // init executor and execute
    let storage = Arc::new(StratusStorage::new(Arc::new(InMemoryTemporaryStorage::default()), Arc::new(pg)));
    let executor = config.init_executor(storage);
    executor.import_external(block, &mut receipts).await.unwrap();

    // get metrics from prometheus
    for query in METRIC_QUERIES {
        // formatting between query groups
        if query.is_empty() {
            println!("\n--------------------");
            continue;
        }

        // get metrics and print them
        // iterate until prometheus returns something
        let url = format!("{}?query={}", docker.prometheus_api_url(), query);

        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() <= deadline {
            let response = reqwest::get(&url).await.unwrap().json::<serde_json::Value>().await.unwrap();
            let results = response.get("data").unwrap().get("result").unwrap().as_array().unwrap();
            if results.is_empty() {
                continue;
            }

            for result in results {
                let value: &str = result.get("value").unwrap().as_array().unwrap().last().unwrap().as_str().unwrap();
                let value: f64 = value.parse().unwrap();

                if query.contains("_count") {
                    println!("{:<64} = {}", query, value);
                } else {
                    let secs = Duration::from_secs_f64(value);
                    println!("{:<64} = {}", query, secs.fancy_duration().truncate(1));
                }
            }
            break;
        }
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
