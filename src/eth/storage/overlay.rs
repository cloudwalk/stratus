// TODO: doc

use std::sync::Arc;

use super::InMemoryStorage;
use crate::eth::storage::EthStorage;

pub struct Overlay {
    _temp: InMemoryStorage,
    _perm: Arc<dyn EthStorage>,
}
