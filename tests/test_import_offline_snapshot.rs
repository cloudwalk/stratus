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
use testcontainers::clients;
use testcontainers::Container;
use testcontainers::RunnableImage;
use testcontainers_modules::postgres::Postgres as PostgresImage;

const TRACKED_METRICS: [&str; 8] = [
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
    let docker = clients::Cli::default();
    let config = init_global_services::<CommonConfig>();

    // init block
    let block_json = include_str!("fixtures/block-292973/block.json");
    let block: ExternalBlock = serde_json::from_str(block_json).unwrap();

    // init receipts
    let receipts_json = include_str!("fixtures/block-292973/receipts.json");
    let mut receipts = HashMap::new();
    for receipt_json in receipts_json.lines() {
        let receipt: ExternalReceipt = serde_json::from_str(receipt_json).unwrap();
        receipts.insert(receipt.hash(), receipt);
    }

    // init snapshot
    let snapshot_json = include_str!("fixtures/block-292973/snapshot.json");
    let snapshot: InMemoryPermanentStorageState = serde_json::from_str(snapshot_json).unwrap();

    // init postgres from snapshot
    let (_pg_container, pg) = populate_postgres(&docker, snapshot).await;
    let storage = Arc::new(StratusStorage::new(Arc::new(InMemoryTemporaryStorage::default()), Arc::new(pg)));

    // init executor and execute
    let executor = config.init_executor(storage);
    executor.import_offline(block, &receipts).await.unwrap();

    // get metrics from prometheus-exporter page because there is no simple way to get them from the code
    // for now just print them
    let metrics = reqwest::get("http://localhost:9000/").await.unwrap().text().await.unwrap();
    for line in metrics.lines() {
        for metric in TRACKED_METRICS {
            if line.starts_with(metric) {
                println!("{}", line);
            }
        }
    }
}

async fn populate_postgres(docker: &clients::Cli, state: InMemoryPermanentStorageState) -> (Container<'_, PostgresImage>, Postgres) {
    // init docker container (this should be extract to a reusable module if going to be used in other tests)
    let pg_image = RunnableImage::from(PostgresImage::default().with_user("postgres").with_password("123").with_db_name("stratus"))
        .with_mapped_port((5432, 5432))
        .with_volume(("./static/schema/001-init.sql", "/docker-entrypoint-initdb.d/001-schema.sql"))
        .with_volume(("./static/schema/002-schema-external-rpc.sql", "/docker-entrypoint-initdb.d/002-schema.sql"))
        .with_tag("16.1");
    let pg_container = docker.run(pg_image);

    // init postgres client
    let pg = Postgres::new("postgres://postgres:123@localhost:5432/stratus").await.unwrap();

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

    (pg_container, pg)
}
