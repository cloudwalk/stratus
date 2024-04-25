mod test_import_external_snapshot_common;

use stratus::eth::storage::InMemoryPermanentStorage;
use stratus::infra::docker::Docker;
use test_import_external_snapshot_common as common;

#[test]
fn test_import_external_snapshot_with_inmemory() {
    let (global_services, block, receipts, snapshot) = common::init_config_and_data(292973);
    global_services.runtime.block_on(async move {
        let docker = Docker::default();
        let _prom_guard = docker.start_prometheus();
        let inmemory = InMemoryPermanentStorage::from_snapshot(snapshot);

        common::execute_test("InMemory", &global_services.config, &docker, inmemory, block, receipts).await;
    });
}
