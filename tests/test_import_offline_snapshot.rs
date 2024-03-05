use std::collections::HashMap;
use std::sync::Arc;

use stratus::config::CommonConfig;
use stratus::eth::primitives::ExternalBlock;
use stratus::eth::primitives::ExternalReceipt;
use stratus::eth::storage::InMemoryPermanentStorage;
use stratus::eth::storage::InMemoryPermanentStorageState;
use stratus::eth::storage::InMemoryTemporaryStorage;
use stratus::eth::storage::StratusStorage;
use stratus::infra::metrics::METRIC_EVM_EXECUTION;
use stratus::infra::metrics::METRIC_EVM_EXECUTION_ACCOUNT_READS;
use stratus::infra::metrics::METRIC_EVM_EXECUTION_SLOT_READS;
use stratus::infra::metrics::METRIC_EXECUTOR_IMPORT_OFFLINE_ACCOUNT_READS;
use stratus::infra::metrics::METRIC_EXECUTOR_IMPORT_OFFLINE_SLOT_READS;
use stratus::init_global_services;

const TRACKED_METRICS: [&str; 5] = [
    METRIC_EVM_EXECUTION,
    METRIC_EVM_EXECUTION_SLOT_READS,
    METRIC_EVM_EXECUTION_ACCOUNT_READS,
    METRIC_EXECUTOR_IMPORT_OFFLINE_ACCOUNT_READS,
    METRIC_EXECUTOR_IMPORT_OFFLINE_SLOT_READS,
];

#[tokio::test]
async fn test_import_offline_snapshot() {
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

    // init storage
    let snapshot_json = include_str!("fixtures/block-292973/snapshot.json");
    let snapshot: InMemoryPermanentStorageState = serde_json::from_str(snapshot_json).unwrap();
    let perm_storage = Arc::new(InMemoryPermanentStorage::from_snapshot(snapshot));
    let storage = Arc::new(StratusStorage::new(Arc::new(InMemoryTemporaryStorage::default()), perm_storage));

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
