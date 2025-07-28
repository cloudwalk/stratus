//! Column Family (CF) versioning.
//!
//! This allows our KV-store to have different versions on the Value.
//!
//! Versions are tested against snapshots to avoid breaking changes.

use std::fmt::Debug;
use std::ops::Deref;
use std::ops::DerefMut;

use anyhow::Context;
#[cfg(feature = "replication")]
use rocksdb::WriteBatch;
use serde::Deserialize;
use serde::Serialize;
use strum::EnumCount;
use strum::IntoStaticStr;
use strum::VariantNames;

use super::types::AccountRocksdb;
use super::types::AddressRocksdb;
use super::types::BlockNumberRocksdb;
use super::types::BlockRocksdb;
#[cfg(feature = "replication")]
use super::types::BytesRocksdb;
use super::types::SlotIndexRocksdb;
use super::types::SlotValueRocksdb;
use crate::eth::primitives::Account;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::SlotValue;
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;
use crate::eth::storage::permanent::rocks::types::old_types_hotfix::OldCfBlocksByNumberValue;
macro_rules! impl_single_version_cf_value {
    ($name:ident, $inner_type:ty, $non_rocks_equivalent: ty) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, EnumCount, VariantNames, IntoStaticStr, fake::Dummy)]
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
#[cfg(feature = "replication")]
impl_single_version_cf_value!(CfReplicationLogsValue, BytesRocksdb, WriteBatch);

impl SerializeDeserializeWithContext for CfAccountSlotsHistoryValue {}
impl SerializeDeserializeWithContext for CfAccountSlotsValue {}
impl SerializeDeserializeWithContext for CfAccountsHistoryValue {}
impl SerializeDeserializeWithContext for CfAccountsValue {}
impl SerializeDeserializeWithContext for CfBlocksByHashValue {}
impl SerializeDeserializeWithContext for CfTransactionsValue {}

// Tuple implementations for composite keys
impl SerializeDeserializeWithContext for (AddressRocksdb, BlockNumberRocksdb) {}
impl SerializeDeserializeWithContext for (AddressRocksdb, SlotIndexRocksdb) {}
impl SerializeDeserializeWithContext for (AddressRocksdb, SlotIndexRocksdb, BlockNumberRocksdb) {}

impl SerializeDeserializeWithContext for CfBlocksByNumberValue {
    fn deserialize_with_context(bytes: &[u8]) -> anyhow::Result<Self>
    where
        Self: for<'de> Deserialize<'de>,
    {
        let res = bincode::deserialize::<Self>(bytes);

        match res {
            Err(_) => Ok(bincode::deserialize::<OldCfBlocksByNumberValue>(bytes)
                .with_context(|| format!("failed to deserialize '{}'", hex_fmt::HexFmt(bytes)))
                .with_context(|| format!("failed to deserialize to type '{}'", std::any::type_name::<Self>()))?
                .into()),
            Ok(ok) => Ok(ok),
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
trait ToCfName {
    const CF_NAME: &'static str;
}

macro_rules! impl_to_cf_name {
    ($type:ident, $cf_name:expr) => {
        impl ToCfName for $type {
            const CF_NAME: &'static str = $cf_name;
        }
    };
}

impl_to_cf_name!(CfAccountsValue, "accounts");
impl_to_cf_name!(CfAccountsHistoryValue, "accounts_history");
impl_to_cf_name!(CfAccountSlotsValue, "account_slots");
impl_to_cf_name!(CfAccountSlotsHistoryValue, "account_slots_history");
impl_to_cf_name!(CfTransactionsValue, "transactions");
impl_to_cf_name!(CfBlocksByNumberValue, "blocks_by_number");
impl_to_cf_name!(CfBlocksByHashValue, "blocks_by_hash");
#[cfg(feature = "replication")]
impl_to_cf_name!(CfReplicationLogsValue, "replication_logs");
/// Test that deserialization works for each variant of the enum.
///
/// This is intended to give an error when the following happens:
///
/// 1. A new variant is added to the enum.
/// 2. A variant is renamed.
/// 3. A variant is removed.
/// 4. A variant is modified.
/// 5. A variant is reordered.
///
/// Here is a breakdown of why, and how to proceed:
///
/// 1. New variants need to be tested, go to the test below and cover it, but watch out for:
///   - You'll need an ENV VAR to create the new snapshot file.
///   - When commiting the change, make sure you're just adding your new snapshot, and not editing others by accident.
/// 2. For renamed variants, because we use bincode, you just need to update the snapshot file.
///   - Rename it locally.
/// 3. Previous variants can't be removed as they break our database, because they won't be able to read the older data.
///   - Don't do it¹.
/// 4. If you modify a variant, the database won't be able to read it anymore.
///   - Don't do it¹.
/// 5. Reordering variants will break deserialization because bincode uses their order to determine the enum tag.
///   - Don't do it¹.
///
/// ¹: if you really want to do it, make sure you can reload your entire database from scratch.
#[cfg(test)]
mod tests {
    use std::env;
    use std::fmt::Debug;
    use std::fs;
    use std::marker::PhantomData;
    use std::path::Path;

    use anyhow::Context;
    use anyhow::Result;
    use anyhow::bail;
    use anyhow::ensure;

    use super::*;
    use crate::ext::not;
    use crate::ext::type_basename;
    use crate::utils::test_utils::glob_to_string_paths;

    /// A drop bomb that guarantees that all variants of an enum have been tested.
    struct EnumCoverageDropBombChecker<CfValue>
    where
        CfValue: VariantNames + ToCfName,
    {
        confirmations: Vec<TestRunConfirmation<CfValue>>,
    }

    impl<CfValue> EnumCoverageDropBombChecker<CfValue>
    where
        CfValue: VariantNames + ToCfName,
    {
        fn new() -> Self {
            Self { confirmations: Vec::new() }
        }

        fn add(&mut self, rhs: TestRunConfirmation<CfValue>) {
            self.confirmations.push(rhs);
        }
    }

    impl<CfValue> Drop for EnumCoverageDropBombChecker<CfValue>
    where
        CfValue: VariantNames + ToCfName,
    {
        fn drop(&mut self) {
            // check for missing confirmations
            for variant_name in CfValue::VARIANTS {
                let found = self.confirmations.iter().find(|confirmation| confirmation.variant_name == *variant_name);

                if found.is_none() {
                    panic!(
                        "TestRunDropBombChecker<{enum_typename}> panic on drop: cf {}: missing test for variant '{}' of enum {enum_typename}",
                        CfValue::CF_NAME,
                        variant_name,
                        enum_typename = type_basename::<CfValue>(),
                    );
                }
            }
        }
    }

    /// A confirmation that a test was run for a specific variant of an enum, used by the drop bomb.
    struct TestRunConfirmation<CfValue> {
        variant_name: &'static str,
        _marker: PhantomData<CfValue>,
    }

    impl<CfValue> TestRunConfirmation<CfValue> {
        fn new(variant_name: &'static str) -> Self {
            Self {
                variant_name,
                _marker: PhantomData,
            }
        }
    }

    fn get_all_bincode_snapshots_from_folder(folder: impl AsRef<str>) -> Result<Vec<String>> {
        let pattern = format!("{}/*.bincode", folder.as_ref());
        glob_to_string_paths(pattern).context("failed to get all bincode snapshots from folder")
    }

    fn load_or_generate_json_fixture<CfValue>(cf_name: &str, _variant_name: &str) -> Result<CfValue>
    where
        CfValue: for<'de> Deserialize<'de> + Serialize + fake::Dummy<fake::Faker> + ToCfName,
    {
        let json_path = format!("tests/fixtures/cf_versions/{cf_name}/{cf_name}.json");
        let json_parent_path = format!("tests/fixtures/cf_versions/{cf_name}");

        // Try to load existing fixture first
        if Path::new(&json_path).exists() {
            let json_content = fs::read_to_string(&json_path).with_context(|| format!("failed to read JSON fixture at {json_path}"))?;
            return serde_json::from_str(&json_content).with_context(|| format!("failed to deserialize CfValue from JSON fixture at {json_path}"));
        }

        // Generate fixture if it doesn't exist and DANGEROUS_UPDATE_SNAPSHOTS is set
        if env::var("DANGEROUS_UPDATE_SNAPSHOTS").is_ok() {
            let generated_fixture = crate::utils::test_utils::fake_first::<CfValue>();
            let json_content =
                serde_json::to_string_pretty(&generated_fixture).with_context(|| format!("failed to serialize generated fixture for {cf_name}"))?;

            fs::create_dir_all(&json_parent_path).with_context(|| format!("failed to create directory {json_parent_path}"))?;
            fs::write(&json_path, &json_content).with_context(|| format!("failed to write JSON fixture to {json_path}"))?;

            return Ok(generated_fixture);
        }

        bail!("JSON fixture at '{json_path}' doesn't exist and DANGEROUS_UPDATE_SNAPSHOTS is not set");
    }

    /// Store snapshots of the current serialization format for each version.
    #[test]
    fn test_snapshot_bincode_deserialization_for_single_version_enums() {
        fn test_deserialization<CfValue>() -> Result<TestRunConfirmation<CfValue>>
        where
            CfValue: for<'de> Deserialize<'de> + Serialize + Clone + Debug + PartialEq + Into<&'static str> + ToCfName + fake::Dummy<fake::Faker>,
        {
            let cf_name = CfValue::CF_NAME;
            // For single version enums, we expect V1 variant
            let variant_name = "V1";
            let expected: CfValue = load_or_generate_json_fixture(cf_name, variant_name)?;

            let snapshot_parent_path = format!("tests/fixtures/cf_versions/{cf_name}");
            let snapshot_path = format!("{snapshot_parent_path}/{variant_name}.bincode");

            // create snapshot if it doesn't exist
            if not(Path::new(&snapshot_path).exists()) {
                // -> CAREFUL WHEN UPDATING SNAPSHOTS <-
                // the snapshots are supposed to prevent you from breaking the DB accidentally
                // the DB must be able to deserialize older versions, and those versions can't change
                // don't reorder variants, remove older variants or modify the data inside existing ones
                // adding a new snapshot for a new variant is safe as long as you don't mess up in the points above
                // -> CAREFUL WHEN UPDATING SNAPSHOTS <-
                if env::var("DANGEROUS_UPDATE_SNAPSHOTS").is_ok() {
                    let serialized = bincode::serialize(&expected)?;
                    fs::create_dir_all(&snapshot_parent_path)?;
                    fs::write(snapshot_path, serialized)?;
                } else {
                    bail!("snapshot file at '{snapshot_path:?}' doesn't exist and DANGEROUS_UPDATE_SNAPSHOTS is not set");
                }
            }

            let snapshots = get_all_bincode_snapshots_from_folder(&snapshot_parent_path)?;

            let [snapshot_path] = snapshots.as_slice() else {
                bail!("expected 1 snapshot, found {}: {snapshots:?}", snapshots.len());
            };

            ensure!(
                snapshot_path == snapshot_path,
                "snapshot path {snapshot_path:?} doesn't match the expected for v1: {snapshot_path:?}"
            );

            let deserialized = bincode::deserialize::<CfValue>(&fs::read(snapshot_path)?)?;
            ensure!(
                expected == deserialized,
                "deserialized value doesn't match expected\n deserialized = {deserialized:?}\n expected = {expected:?}",
            );

            Ok(TestRunConfirmation::new(variant_name))
        }

        let mut accounts_checker = EnumCoverageDropBombChecker::<CfAccountsValue>::new();
        let mut accounts_history_checker = EnumCoverageDropBombChecker::<CfAccountsHistoryValue>::new();
        let mut account_slots_checker = EnumCoverageDropBombChecker::<CfAccountSlotsValue>::new();
        let mut account_slots_history_checker = EnumCoverageDropBombChecker::<CfAccountSlotsHistoryValue>::new();
        let mut transactions_checker = EnumCoverageDropBombChecker::<CfTransactionsValue>::new();
        let mut blocks_by_number_checker = EnumCoverageDropBombChecker::<CfBlocksByNumberValue>::new();
        let mut blocks_by_hash_checker = EnumCoverageDropBombChecker::<CfBlocksByHashValue>::new();
        #[cfg(feature = "replication")]
        let mut replication_logs_checker = EnumCoverageDropBombChecker::<CfReplicationLogsValue>::new();

        accounts_checker.add(test_deserialization::<CfAccountsValue>().unwrap());
        accounts_history_checker.add(test_deserialization::<CfAccountsHistoryValue>().unwrap());
        account_slots_checker.add(test_deserialization::<CfAccountSlotsValue>().unwrap());
        account_slots_history_checker.add(test_deserialization::<CfAccountSlotsHistoryValue>().unwrap());
        transactions_checker.add(test_deserialization::<CfTransactionsValue>().unwrap());
        blocks_by_number_checker.add(test_deserialization::<CfBlocksByNumberValue>().unwrap());
        blocks_by_hash_checker.add(test_deserialization::<CfBlocksByHashValue>().unwrap());
        #[cfg(feature = "replication")]
        replication_logs_checker.add(test_deserialization::<CfReplicationLogsValue>().unwrap());
    }
}
