use std::fmt::Debug;

use ethereum_types::H64;

use crate::eth::primitives::MinerNonce;
use crate::gen_newtype_from;

#[derive(Debug, Clone, Default, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct MinerNonceRocksdb(H64);

gen_newtype_from!(self = MinerNonceRocksdb, other = H64, [u8; 8], MinerNonce);

impl From<MinerNonceRocksdb> for MinerNonce {
    fn from(value: MinerNonceRocksdb) -> Self {
        value.0.into()
    }
}
