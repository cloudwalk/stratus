mod inmemory_account;
mod inmemory_history;
mod inmemory_permanent;
#[allow(clippy::module_inception)]
mod inmemory_state;
mod inmemory_temporary;

/// The inmemory storage is split into two structs. InMemoryStoragePermanent and
/// InMemoryStorageTemporary to facilitate debugging when using the inmemory storage
/// for both temp and perm contexts.
pub use inmemory_history::InMemoryHistory;
pub use inmemory_permanent::InMemoryStoragePermanent;
pub use inmemory_temporary::InMemoryStorageTemporary;
