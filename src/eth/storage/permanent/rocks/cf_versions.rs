//! Column Family (CF) versioning.
//!
//! This allows our KV-store to have different versions on the Value.
//!
//! Versions are tested against snapshots to avoid breaking changes.

use std::fmt::Debug;
use std::ops::Deref;
use std::ops::DerefMut;

use serde::Deserialize;
use serde::Serialize;
use stratus_macros::FakeEnum;
use strum::EnumCount;
use strum::IntoStaticStr;
use strum::VariantNames;

use super::types::AccountRocksdb;
use super::types::BlockNumberRocksdb;
use super::types::BlockRocksdb;
use super::types::SlotValueRocksdb;
use crate::eth::primitives::Account;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::SlotValue;
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;
#[cfg(test)]
use crate::eth::storage::permanent::rocks::test_utils::FakeEnum;
use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;

macro_rules! impl_single_version_cf_value {
    ($name:ident, $inner_type:ty, $non_rocks_equivalent: ty) => {
        #[derive(
            Debug, Clone, PartialEq, Serialize, Deserialize, EnumCount, VariantNames, IntoStaticStr, fake::Dummy, bincode::Encode, bincode::Decode, FakeEnum,
        )]
        #[fake_enum(generate = "crate::utils::test_utils::fake_first")]
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
impl_single_version_cf_value!(CfBlockChangesValue, BlockChangesRocksdb, ());

impl SerializeDeserializeWithContext for CfAccountSlotsHistoryValue {}
impl SerializeDeserializeWithContext for CfAccountSlotsValue {}
impl SerializeDeserializeWithContext for CfAccountsHistoryValue {}
impl SerializeDeserializeWithContext for CfAccountsValue {}
impl SerializeDeserializeWithContext for CfBlocksByHashValue {}
impl SerializeDeserializeWithContext for CfTransactionsValue {}
impl SerializeDeserializeWithContext for CfBlockChangesValue {}
impl SerializeDeserializeWithContext for CfBlocksByNumberValue {}

#[cfg(test)]
mod cf_names {
    use super::*;
    use crate::eth::storage::permanent::rocks::test_utils::ToCfName;
    use crate::impl_to_cf_name;

    impl_to_cf_name!(CfAccountsValue, "accounts");
    impl_to_cf_name!(CfAccountsHistoryValue, "accounts_history");
    impl_to_cf_name!(CfAccountSlotsValue, "account_slots");
    impl_to_cf_name!(CfAccountSlotsHistoryValue, "account_slots_history");
    impl_to_cf_name!(CfTransactionsValue, "transactions");
    impl_to_cf_name!(CfBlocksByNumberValue, "blocks_by_number");
    impl_to_cf_name!(CfBlocksByHashValue, "blocks_by_hash");
    impl_to_cf_name!(CfBlockChangesValue, "block_changes");
}

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
    use anyhow::Context;

    use super::*;
    use crate::eth::storage::permanent::rocks::test_utils::EnumCoverageDropBombChecker;
    use crate::eth::storage::permanent::rocks::test_utils::{self};

    /// Store snapshots of the current serialization format for each version.
    #[test]
    fn test_snapshot_bincode_deserialization_for_version_enums() -> anyhow::Result<()> {
        let mut accounts_checker = EnumCoverageDropBombChecker::<CfAccountsValue>::new();
        let mut accounts_history_checker = EnumCoverageDropBombChecker::<CfAccountsHistoryValue>::new();
        let mut account_slots_checker = EnumCoverageDropBombChecker::<CfAccountSlotsValue>::new();
        let mut account_slots_history_checker = EnumCoverageDropBombChecker::<CfAccountSlotsHistoryValue>::new();
        let mut transactions_checker = EnumCoverageDropBombChecker::<CfTransactionsValue>::new();
        let mut blocks_by_number_checker = EnumCoverageDropBombChecker::<CfBlocksByNumberValue>::new();
        let mut blocks_by_hash_checker = EnumCoverageDropBombChecker::<CfBlocksByHashValue>::new();

        test_utils::verify_fixtures(&mut accounts_checker, "cf_versions").context("failed to verify variants for CfAccountsValue")?;
        test_utils::verify_fixtures(&mut accounts_history_checker, "cf_versions").context("failed to verify variants for CfAccountsHistoryValue")?;
        test_utils::verify_fixtures(&mut account_slots_checker, "cf_versions").context("failed to verify variants for CfAccountSlotsValue")?;
        test_utils::verify_fixtures(&mut account_slots_history_checker, "cf_versions").context("failed to verify variants for CfAccountSlotsHistoryValue")?;
        test_utils::verify_fixtures(&mut transactions_checker, "cf_versions").context("failed to verify variants for CfTransactionsValue")?;
        test_utils::verify_fixtures(&mut blocks_by_number_checker, "cf_versions").context("failed to verify variants for CfBlocksByNumberValue")?;
        test_utils::verify_fixtures(&mut blocks_by_hash_checker, "cf_versions").context("failed to verify variants for CfBlocksByHashValue")?;

        Ok(())
    }
}
