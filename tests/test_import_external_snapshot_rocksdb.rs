mod test_import_external_snapshot_common;

use stratus::eth::primitives::BlockNumber;
use stratus::eth::storage::PermanentStorage;
use stratus::eth::storage::RocksPermanentStorage;
use stratus::infra::docker::Docker;
use test_import_external_snapshot_common as common;

#[tokio::test]
async fn test_import_external_snapshot_with_rocksdb() {
    let docker = Docker::default();
    let _prom_guard = docker.start_prometheus();

    let (config, block, receipts, snapshot) = common::init_config_and_data();
    let (accounts, slots) = common::filter_accounts_and_slots(snapshot);

    let rocks = RocksPermanentStorage::new().await.unwrap();
    rocks.save_accounts(accounts).await.unwrap();
    rocks.state.write_slots(slots, BlockNumber::ZERO);

    common::execute_test("RocksDB", &config, &docker, rocks, block, receipts).await;
}
