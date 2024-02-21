use async_trait::async_trait;

/// EVM storage operations.
#[async_trait]
pub trait EthStorage: Send + Sync {}
