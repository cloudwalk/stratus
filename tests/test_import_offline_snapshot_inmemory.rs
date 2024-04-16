mod test_import_offline_snapshot_common;

use stratus::eth::storage::InMemoryPermanentStorage;
use stratus::infra::docker::Docker;
use test_import_offline_snapshot_common as common;

#[tokio::test]
async fn test_import_offline_snapshot_with_inmemory() {
    let docker = Docker::default();
    let _prom_guard = docker.start_prometheus();

    let (config, block, receipts, snapshot) = common::init_config_and_data();
    let inmemory = InMemoryPermanentStorage::from_snapshot(snapshot);

    common::execute_test("InMemory", &config, &docker, inmemory, block, receipts).await;
}
