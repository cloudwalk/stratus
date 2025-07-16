pub use inmemory::InMemoryTemporaryStorage;

use crate::eth::{primitives::BlockNumber, storage::temporary::inmemory::call::TxCount};

mod call;
mod inmemory;
mod transaction;

pub enum ReadKind {
    Call((BlockNumber, TxCount)),
    Transaction
}
