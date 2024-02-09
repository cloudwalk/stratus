// TODO: doc

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use async_trait::async_trait;
use indexmap::IndexMap;
use metrics::atomics::AtomicU64;
use tokio::sync::RwLock;
use tokio::sync::RwLockReadGuard;
use tokio::sync::RwLockWriteGuard;

use crate::eth::storage::inmemory::InMemoryAccount;
use crate::eth::miner::BlockMiner;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
use crate::eth::primitives::Execution;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::ExecutionConflicts;
use crate::eth::primitives::ExecutionConflictsBuilder;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::inmemory::InMemoryHistory;
use crate::eth::storage::test_accounts;
use crate::eth::storage::EthStorage;
use crate::eth::storage::EthStorageError;
use super::InMemoryStorage;

pub struct Overlay {
    temp: InMemoryStorage,
    perm: Arc<dyn EthStorage>,
}
