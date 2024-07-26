use std::ops::Deref;
use std::ops::DerefMut;

use serde::Deserialize;
use serde::Serialize;
use strum::EnumCount;

use super::types::AccountRocksdb;
use super::types::BlockNumberRocksdb;
use super::types::BlockRocksdb;
use super::types::SlotValueRocksdb;
use crate::eth::primitives::Account;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::SlotValue;

macro_rules! impl_single_version_cf_value {
    ($name:ident, $inner_type:ty, $non_rocks_equivalent: ty) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, EnumCount)]
        pub enum $name {
            V1($inner_type),
        }

        impl $name {
            #[allow(dead_code)]
            pub fn into_inner(self) -> $inner_type {
                match self {
                    Self::V1(v1) => v1,
                }
            }
        }

        // `From` conversion should only exist for values with a single version
        static_assertions::const_assert_eq!($name::COUNT, 1);

        // implement `From` for the v1 type.
        impl From<$inner_type> for $name {
            fn from(v1: $inner_type) -> Self {
                Self::V1(v1)
            }
        }

        impl Deref for $name {
            type Target = $inner_type;
            fn deref(&self) -> &Self::Target {
                match self {
                    Self::V1(v1) => v1,
                }
            }
        }

        impl DerefMut for $name {
            fn deref_mut(&mut self) -> &mut Self::Target {
                match self {
                    Self::V1(v1) => v1,
                }
            }
        }

        // Do `$non_rocks_equivalent -> $inner_type -> $name` in one conversion.
        impl From<$non_rocks_equivalent> for $name {
            fn from(value: $non_rocks_equivalent) -> Self {
                let value = <$inner_type>::from(value);
                Self::V1(value)
            }
        }
    };
}

impl_single_version_cf_value!(CfAccountsValue, AccountRocksdb, Account);
impl_single_version_cf_value!(CfAccountsHistoryValue, AccountRocksdb, Account);
impl_single_version_cf_value!(CfAccountSlotsValue, SlotValueRocksdb, SlotValue);
impl_single_version_cf_value!(CfAccountSlotsHistoryValue, SlotValueRocksdb, SlotValue);
impl_single_version_cf_value!(CfTransactionsValue, BlockNumberRocksdb, BlockNumber);
impl_single_version_cf_value!(CfBlocksByNumberValue, BlockRocksdb, Block);
impl_single_version_cf_value!(CfBlocksByHashValue, BlockNumberRocksdb, BlockNumber);
impl_single_version_cf_value!(CfLogsValue, BlockNumberRocksdb, BlockNumber);

#[cfg(test)]
mod tests {
    use std::env;
    use std::fmt::Debug;
    use std::fs;
    use std::marker::PhantomData;
    use std::ops;
    use std::path::Path;

    use anyhow::bail;
    use anyhow::ensure;
    use anyhow::Context;
    use anyhow::Result;
    use fake::Dummy;
    use fake::Faker;
    use itertools::Itertools;

    use super::*;
    use crate::ext::not;
    use crate::ext::type_basename;
    use crate::utils::test_utils::fake_first;
    use crate::utils::test_utils::glob_to_string_paths;

    fn get_snapshot_folder_path_and_parent_path(name: &str) -> String {
        format!("tests/fixtures/cf_versions/{name}")
    }

    fn get_all_bincode_snapshots_from_folder(folder: impl AsRef<str>) -> Result<Vec<String>> {
        let pattern = format!("{}/*.bincode", folder.as_ref());
        glob_to_string_paths(pattern).context("failed to get all bincode snapshots from folder")
    }

    struct TestRunDropBombChecker<CfValue>
    where
        CfValue: EnumCount,
    {
        name: String,
        certificates: Vec<TestRunConfirmation<CfValue>>,
        _marker: PhantomData<CfValue>,
    }

    impl<CfValue> TestRunDropBombChecker<CfValue>
    where
        CfValue: EnumCount,
    {
        fn new(name: impl ToString) -> Self {
            Self {
                name: name.to_string(),
                certificates: Vec::new(),
                _marker: PhantomData,
            }
        }
    }

    impl<CfValue> ops::BitOrAssign<TestRunConfirmation<CfValue>> for TestRunDropBombChecker<CfValue>
    where
        CfValue: EnumCount,
    {
        fn bitor_assign(&mut self, rhs: TestRunConfirmation<CfValue>) {
            self.certificates.push(rhs);
        }
    }

    // use CfValue::COUNT to check if all certificates were there
    impl<CfValue> Drop for TestRunDropBombChecker<CfValue>
    where
        CfValue: EnumCount,
    {
        fn drop(&mut self) {
            let variants = CfValue::COUNT;

            for variant in 1..=variants {
                let found = self.certificates.iter().find(|certificate| certificate.version_number == variant);

                if found.is_none() {
                    panic!(
                        "TestRunDropBombChecker panic on drop: : missing certificate for variant {} of {}",
                        variant,
                        type_basename::<CfValue>()
                    );
                }
            }
        }
    }

    struct TestRunConfirmation<CfValue> {
        version_number: usize,
        _marker: PhantomData<CfValue>,
    }

    impl<CfValue> TestRunConfirmation<CfValue>
    where
        CfValue: EnumCount,
    {
        fn new(version_number: usize) -> Self {
            Self {
                version_number,
                _marker: PhantomData,
            }
        }
    }

    /// Store snapshots of the current serialization format for each version.
    #[test]
    fn test_snapshot_bincode_deserialization_for_single_version_enums() {
        fn create_new_snapshots<CfValue, Inner>(name: &str) -> Result<()>
        where
            CfValue: From<Inner> + Serialize + EnumCount,
            Inner: Dummy<Faker>,
        {
            let last_variant_number = <CfValue as EnumCount>::COUNT;
            let snapshot_parent_path = get_snapshot_folder_path_and_parent_path(name);
            let snapshot_path = format!("{snapshot_parent_path}/v{last_variant_number}.bincode");
            let snapshot_path = Path::new(&snapshot_path);

            if not(snapshot_path.exists()) {
                if env::var("GEN_NEW_VARIANT_SNAPSHOT").is_ok() {
                    let expected: CfValue = fake_first::<Inner>().into();
                    let serialized = bincode::serialize(&expected)?;
                    fs::create_dir_all(snapshot_parent_path)?;
                    fs::write(snapshot_path, serialized)?;
                } else {
                    bail!("snapshot file at '{snapshot_path:?}' doesn't exist and GEN_NEW_VARIANT_SNAPSHOT is not set");
                }
            }
            Ok(())
        }

        fn check_if_snapshot_files_exist<CfValue>(name: &str) -> Result<()>
        where
            CfValue: EnumCount,
        {
            let variant_count = CfValue::COUNT;
            let folder = get_snapshot_folder_path_and_parent_path(name);
            let snapshots = get_all_bincode_snapshots_from_folder(&folder)?;
            let filenames = snapshots.iter().map(|path| path.split('/').next_back().unwrap_or(path.as_str())).collect_vec();

            for i in 1..=variant_count {
                let filename = format!("v{i}.bincode");
                ensure!(filenames.contains(&filename.as_str()), "missing snapshot file {filename} for variant {i}");
            }

            let path_past_last_version = format!("{folder}/v{}.bincode", variant_count + 1);
            ensure!(
                not(Path::new(&path_past_last_version).exists()),
                "found snapshot past last version: '{path_past_last_version}', note that removing a version is a breaking change!",
            );

            Ok(())
        }

        fn test_deserialization<CfValue, Inner, F>(inner_to_cf_value: F, variant_number: usize, name: &str) -> Result<TestRunConfirmation<CfValue>>
        where
            CfValue: From<Inner> + for<'de> Deserialize<'de> + Debug + EnumCount + PartialEq,
            Inner: Dummy<Faker>,
            F: FnOnce(Inner) -> CfValue,
        {
            let expected: CfValue = inner_to_cf_value(fake_first::<Inner>());

            if variant_number > CfValue::COUNT {
                bail!("enum '{}' doesn't have the {variant_number}th variant", type_basename::<CfValue>());
            }

            let snapshot_parent_path = get_snapshot_folder_path_and_parent_path(name);
            let expected_snapshot_path = format!("{snapshot_parent_path}/v{variant_number}.bincode");
            let snapshots = get_all_bincode_snapshots_from_folder(&snapshot_parent_path)?;

            let [snapshot_path] = snapshots.as_slice() else {
                bail!("expected 1 snapshot, found {}: {snapshots:?}", snapshots.len());
            };

            ensure!(
                *snapshot_path == expected_snapshot_path,
                "snapshot path {snapshot_path:?} doesn't match the expected for v1: {expected_snapshot_path:?}"
            );

            let deserialized = bincode::deserialize::<CfValue>(&fs::read(&expected_snapshot_path)?)?;
            ensure!(
                expected == deserialized,
                "deserialized value doesn't match expected\n deserialized = {deserialized:?}\n expected = {expected:?}",
            );
            Ok(TestRunConfirmation::new(variant_number))
        }

        create_new_snapshots::<CfAccountsValue, AccountRocksdb>("accounts").unwrap();
        create_new_snapshots::<CfAccountsHistoryValue, AccountRocksdb>("accounts_history").unwrap();
        create_new_snapshots::<CfAccountSlotsValue, SlotValueRocksdb>("account_slots").unwrap();
        create_new_snapshots::<CfAccountSlotsHistoryValue, SlotValueRocksdb>("account_slots_history").unwrap();
        create_new_snapshots::<CfTransactionsValue, BlockNumberRocksdb>("transactions").unwrap();
        create_new_snapshots::<CfBlocksByNumberValue, BlockRocksdb>("blocks_by_number").unwrap();
        create_new_snapshots::<CfBlocksByHashValue, BlockNumberRocksdb>("blocks_by_hash").unwrap();
        create_new_snapshots::<CfLogsValue, BlockNumberRocksdb>("logs").unwrap();

        check_if_snapshot_files_exist::<CfAccountsValue>("accounts").unwrap();
        check_if_snapshot_files_exist::<CfAccountsHistoryValue>("accounts_history").unwrap();
        check_if_snapshot_files_exist::<CfAccountSlotsValue>("account_slots").unwrap();
        check_if_snapshot_files_exist::<CfAccountSlotsHistoryValue>("account_slots_history").unwrap();
        check_if_snapshot_files_exist::<CfTransactionsValue>("transactions").unwrap();
        check_if_snapshot_files_exist::<CfBlocksByNumberValue>("blocks_by_number").unwrap();
        check_if_snapshot_files_exist::<CfBlocksByHashValue>("blocks_by_hash").unwrap();
        check_if_snapshot_files_exist::<CfLogsValue>("logs").unwrap();

        let mut accounts_checker = TestRunDropBombChecker::new("accounts");
        let mut accounts_history_checker = TestRunDropBombChecker::new("accounts_history");
        let mut account_slots_checker = TestRunDropBombChecker::new("account_slots");
        let mut account_slots_history_checker = TestRunDropBombChecker::new("account_slots_history");
        let mut transactions_checker = TestRunDropBombChecker::new("transactions");
        let mut blocks_by_number_checker = TestRunDropBombChecker::new("blocks_by_number");
        let mut blocks_by_hash_checker = TestRunDropBombChecker::new("blocks_by_hash");
        let mut logs_checker = TestRunDropBombChecker::new("logs");

        accounts_checker |= test_deserialization::<_, AccountRocksdb, _>(CfAccountsValue::V1, 1, "accounts").unwrap();
        accounts_history_checker |= test_deserialization::<_, AccountRocksdb, _>(CfAccountsHistoryValue::V1, 1, "accounts_history").unwrap();
        account_slots_checker |= test_deserialization::<_, SlotValueRocksdb, _>(CfAccountSlotsValue::V1, 1, "account_slots").unwrap();
        account_slots_history_checker |= test_deserialization::<_, SlotValueRocksdb, _>(CfAccountSlotsHistoryValue::V1, 1, "account_slots_history").unwrap();
        transactions_checker |= test_deserialization::<_, BlockNumberRocksdb, _>(CfTransactionsValue::V1, 1, "transactions").unwrap();
        blocks_by_number_checker |= test_deserialization::<_, BlockRocksdb, _>(CfBlocksByNumberValue::V1, 1, "blocks_by_number").unwrap();
        blocks_by_hash_checker |= test_deserialization::<_, BlockNumberRocksdb, _>(CfBlocksByHashValue::V1, 1, "blocks_by_hash").unwrap();
        logs_checker |= test_deserialization::<_, BlockNumberRocksdb, _>(CfLogsValue::V1, 1, "logs").unwrap();
    }
}
