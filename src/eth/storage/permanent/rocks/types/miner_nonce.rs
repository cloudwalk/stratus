use std::fmt::Debug;

use alloy_primitives::B64;

use crate::eth::primitives::MinerNonce;
use crate::eth::storage::permanent::rocks::cf_versions::SerializeDeserializeWithContext;
use crate::gen_newtype_from;

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub struct MinerNonceRocksdb([u8; 8]);

gen_newtype_from!(self = MinerNonceRocksdb, other = B64, [u8; 8], MinerNonce);

impl From<MinerNonceRocksdb> for MinerNonce {
    fn from(value: MinerNonceRocksdb) -> Self {
        value.0.into()
    }
}

impl SerializeDeserializeWithContext for MinerNonceRocksdb {}
