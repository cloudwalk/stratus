use std::fmt::Debug;

use ethereum_types::U256;

use crate::eth::primitives::Difficulty;
use crate::gen_newtype_from;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct DifficultyRocksdb(U256);

gen_newtype_from!(self = DifficultyRocksdb, other = U256);

impl From<DifficultyRocksdb> for Difficulty {
    fn from(value: DifficultyRocksdb) -> Self {
        value.0.into()
    }
}

impl From<Difficulty> for DifficultyRocksdb {
    fn from(value: Difficulty) -> Self {
        U256::from(value).into()
    }
}
