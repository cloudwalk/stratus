use async_trait::async_trait;

use crate::eth::primitives::Account;

/// EVM storage operations.
#[async_trait]
pub trait EthStorage: Send + Sync {}
