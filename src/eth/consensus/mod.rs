pub mod raft;
pub mod simple_consensus;
use async_trait::async_trait;

use crate::eth::primitives::Bytes;
use crate::eth::primitives::Hash;

#[async_trait]
pub trait Consensus: Send + Sync {
    async fn should_serve(&self) -> bool;
    fn should_forward(&self) -> bool;
    async fn forward(&self, transaction: Bytes) -> anyhow::Result<Hash>;
}
