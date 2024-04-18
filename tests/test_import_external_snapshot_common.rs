#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use fancy_duration::AsFancyDuration;
use itertools::Itertools;
use stratus::config::IntegrationTestConfig;
use stratus::eth::primitives::Account;
use stratus::eth::primitives::Address;
use stratus::eth::primitives::ExternalBlock;
use stratus::eth::primitives::ExternalReceipts;
use stratus::eth::primitives::Slot;
use stratus::eth::primitives::StoragePointInTime;
use stratus::eth::storage::InMemoryPermanentStorageState;
use stratus::eth::storage::InMemoryTemporaryStorage;
use stratus::eth::storage::PermanentStorage;
use stratus::eth::storage::StratusStorage;
use stratus::infra::docker::Docker;
use stratus::init_global_services;
#[cfg(feature = "metrics")]
mod m {
    pub use const_format::formatcp;
    pub use stratus::infra::metrics::METRIC_EVM_EXECUTION;
    pub use stratus::infra::metrics::METRIC_EVM_EXECUTION_SLOT_READS_CACHED;
    pub use stratus::infra::metrics::METRIC_STORAGE_COMMIT;
    pub use stratus::infra::metrics::METRIC_STORAGE_READ_ACCOUNT;
    pub use stratus::infra::metrics::METRIC_STORAGE_READ_SLOT;
    pub use stratus::infra::metrics::METRIC_STORAGE_READ_SLOTS;
}

#[cfg(feature = "metrics")]
const METRIC_QUERIES: [&str; 46] = [
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
    "* SLOTS (count)",
    m::formatcp!("{}_sum{{}}", m::METRIC_EVM_EXECUTION_SLOT_READS_CACHED),
    m::formatcp!("{}_count{{}}", m::METRIC_STORAGE_READ_SLOTS),
    m::formatcp!("sum({}_count)", m::METRIC_STORAGE_READ_SLOT),
    m::formatcp!("{}_count{{found_at='temporary'}}", m::METRIC_STORAGE_READ_SLOT),
    m::formatcp!("{}_count{{found_at='permanent'}}", m::METRIC_STORAGE_READ_SLOT),
    m::formatcp!("{}_count{{found_at='default'}}", m::METRIC_STORAGE_READ_SLOT),
    "* SLOTS (cumulative)",
    m::formatcp!("sum({}_sum)", m::METRIC_STORAGE_READ_SLOTS),
    m::formatcp!("sum({}_sum)", m::METRIC_STORAGE_READ_SLOT),
    m::formatcp!("{}_sum{{found_at='temporary'}}", m::METRIC_STORAGE_READ_SLOT),
    m::formatcp!("{}_sum{{found_at='permanent'}}", m::METRIC_STORAGE_READ_SLOT),
    m::formatcp!("{}_sum{{found_at='default'}}", m::METRIC_STORAGE_READ_SLOT),
    "* SLOTS (P100)",
    m::formatcp!("{}{{found_at='temporary', quantile='1'}}", m::METRIC_STORAGE_READ_SLOT),
    m::formatcp!("{}{{found_at='permanent', quantile='1'}}", m::METRIC_STORAGE_READ_SLOT),
    m::formatcp!("{}{{found_at='default', quantile='1'}}", m::METRIC_STORAGE_READ_SLOT),
    "* SLOTS (P95)",
    m::formatcp!("{}{{found_at='temporary', quantile='0.95'}}", m::METRIC_STORAGE_READ_SLOT),
    m::formatcp!("{}{{found_at='permanent', quantile='0.95'}}", m::METRIC_STORAGE_READ_SLOT),
    m::formatcp!("{}{{found_at='default', quantile='0.95'}}", m::METRIC_STORAGE_READ_SLOT),
    "* COMMIT",
    m::formatcp!("{}{{quantile='1'}}", m::METRIC_STORAGE_COMMIT),
];

#[cfg(not(feature = "metrics"))]
const METRIC_QUERIES: [&str; 0] = [];

// -----------------------------------------------------------------------------
// Data initialization
// -----------------------------------------------------------------------------
pub fn init_config_and_data() -> (IntegrationTestConfig, ExternalBlock, ExternalReceipts, InMemoryPermanentStorageState) {
    // init config
    let mut config = init_global_services::<IntegrationTestConfig>();
    config.executor.chain_id = 2009;
    config.executor.num_evms = 8;

    // init block data
    let block_json = include_str!("fixtures/snapshots/292973/block.json");
    let block: ExternalBlock = serde_json::from_str(block_json).unwrap();

    // init receipts data
    let receipts_json = include_str!("fixtures/snapshots/292973/receipts.json");
    let receipts: ExternalReceipts = serde_json::from_str(receipts_json).unwrap();

    // init snapshot data
    let snapshot_json = include_str!("fixtures/snapshots/292973/snapshot.json");
    let snapshot: InMemoryPermanentStorageState = serde_json::from_str(snapshot_json).unwrap();

    (config, block, receipts, snapshot)
}

pub fn filter_accounts_and_slots(snapshot: InMemoryPermanentStorageState) -> (Vec<Account>, Vec<(Address, Slot)>) {
    // filter and convert accounts
    let accounts = snapshot.accounts.values().map(|a| a.to_account(&StoragePointInTime::Present)).collect_vec();

    // filter and convert slots
    let mut slots = Vec::new();
    for account in snapshot.accounts.values() {
        for slot_history in account.slots.values() {
            let slot = slot_history.get_current();
            slots.push((account.address.clone(), slot));
        }
    }

    (accounts, slots)
}

// -----------------------------------------------------------------------------
// Test execution
// -----------------------------------------------------------------------------
pub async fn execute_test(
    test_name: &str,
    // services
    config: &IntegrationTestConfig,
    docker: &Docker,
    perm_storage: impl PermanentStorage + 'static,
    // data
    block: ExternalBlock,
    receipts: ExternalReceipts,
) {
    println!("Executing: {}", test_name);

    // restart prometheus, so the metrics are reset

    // init executor and execute
    let storage = StratusStorage::new(Arc::new(InMemoryTemporaryStorage::new()), Arc::new(perm_storage));
    let executor = config.executor.init(Arc::new(storage));
    executor.import_external_to_perm(block, &receipts).await.unwrap();

    // get metrics from prometheus (sleep to ensure prometheus collected)
    tokio::time::sleep(Duration::from_secs(5)).await;

    println!("{}\n{}\n{}", "=".repeat(80), test_name, "=".repeat(80));
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

            if query.contains("_count") || query.contains("_cached") {
                println!("{:<70} = {}", query, value);
            } else {
                let secs = Duration::from_secs_f64(value);
                println!("{:<70} = {}", query, secs.fancy_duration().truncate(2));
            }
        }
    }
}
