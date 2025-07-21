use std::fmt::Debug;

use crate::eth::primitives::Difficulty;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
#[serde(transparent)]
pub struct DifficultyRocksdb([u64; 4]);

impl From<DifficultyRocksdb> for Difficulty {
    fn from(value: DifficultyRocksdb) -> Self {
        value.0.into()
    }
}

impl From<Difficulty> for DifficultyRocksdb {
    fn from(value: Difficulty) -> Self {
        Self(value.0.into_limbs())
    }
}
