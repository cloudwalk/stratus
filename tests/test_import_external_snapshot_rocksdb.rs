mod test_import_external_snapshot_common;

#[cfg(feature = "rocks")]
pub mod rocks_test {
    use stratus::eth::primitives::BlockNumber;
    use stratus::eth::storage::PermanentStorage;
    use stratus::eth::storage::RocksPermanentStorage;
    use stratus::infra::docker::Docker;

    use super::test_import_external_snapshot_common as common;

    #[test]
    fn test_import_external_snapshot_with_rocksdb() {
        let (global_services, block, receipts, snapshot) = common::init_config_and_data();
        global_services.runtime.block_on(async move {
            let docker = Docker::default();
            let _prom_guard = docker.start_prometheus();

            let (accounts, slots) = common::filter_accounts_and_slots(snapshot);

            let rocks = RocksPermanentStorage::new().await.unwrap();
            rocks.save_accounts(accounts).await.unwrap();
            rocks.state.write_slots(slots, BlockNumber::ZERO);

            common::execute_test("RocksDB", &global_services.config, &docker, rocks, block, receipts).await;
        });
    }
}
