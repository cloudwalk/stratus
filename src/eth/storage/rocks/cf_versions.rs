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
        #[derive(Debug, Clone, Serialize, Deserialize, EnumCount)]
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

        // Implement `From` for the v1 type.
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
