use std::collections::HashMap;

use anyhow::Ok;
use async_trait::async_trait;
use parity_tokio_ipc::Connection;
use parity_tokio_ipc::Endpoint;
use tokio::sync::Mutex;

use crate::config::PermanentStorageKind;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotIndexes;
use crate::eth::primitives::SlotSample;
use crate::eth::primitives::SlotValue;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::PermanentStorage;
use crate::eth::storage::PermanentStorageIpcRequest;
use crate::eth::storage::PermanentStorageIpcResponse;
use crate::eth::storage::StorageError;
use crate::infra::IpcClient;
use crate::log_and_err;

pub struct IpcPermanentStorage {
    client: Mutex<IpcClient<Connection>>,
}

impl IpcPermanentStorage {
    pub async fn new() -> anyhow::Result<Self> {
        let socket = "./data/perm-storage.sock".to_string();
        tracing::info!(%socket, "creating ipc permanent storage");

        let conn = Endpoint::connect(socket).await?;
        let client = IpcClient::new(conn);
        Ok(Self { client: Mutex::new(client) })
    }

    async fn request(&self, request: PermanentStorageIpcRequest) -> anyhow::Result<PermanentStorageIpcResponse> {
        let mut client = self.client.lock().await;

        // request / response
        client.write(request).await?;
        let response = client.read::<PermanentStorageIpcResponse>().await?;

        // handle response error
        if let PermanentStorageIpcResponse::Error(e) = response {
            return log_and_err!(payload = e, "error response from ipc permanent storage");
        }

        Ok(response)
    }
}

#[async_trait]
impl PermanentStorage for IpcPermanentStorage {
    fn kind(&self) -> PermanentStorageKind {
        PermanentStorageKind::Ipc
    }

    async fn allocate_evm_thread_resources(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn set_mined_block_number(&self, _number: BlockNumber) -> anyhow::Result<()> {
        todo!()
    }

    async fn read_mined_block_number(&self) -> anyhow::Result<BlockNumber> {
        let request = PermanentStorageIpcRequest::ReadMinedBlockNumber;
        match self.request(request).await? {
            PermanentStorageIpcResponse::ReadMinedBlockNumber(number) => Ok(number),
            resp => return log_and_err!(payload = resp, "unexpected response"),
        }
    }

    async fn increment_block_number(&self) -> anyhow::Result<BlockNumber> {
        todo!()
    }

    async fn read_account(&self, _address: &Address, _point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Account>> {
        todo!()
    }

    async fn read_slot(&self, _address: &Address, _index: &SlotIndex, _point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>> {
        todo!()
    }

    async fn read_slots(
        &self,
        _address: &Address,
        _indexes: &SlotIndexes,
        _point_in_time: &StoragePointInTime,
    ) -> anyhow::Result<HashMap<SlotIndex, SlotValue>> {
        todo!()
    }

    async fn read_block(&self, selection: &BlockSelection) -> anyhow::Result<Option<Block>> {
        let request = PermanentStorageIpcRequest::ReadBlock(selection.clone());
        match self.request(request).await? {
            PermanentStorageIpcResponse::ReadBlock(block) => Ok(block),
            resp => return log_and_err!(payload = resp, "unexpected response"),
        }
    }

    async fn read_mined_transaction(&self, _hash: &Hash) -> anyhow::Result<Option<TransactionMined>> {
        todo!()
    }

    async fn read_logs(&self, _filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        todo!()
    }

    async fn save_block(&self, _block: Block) -> anyhow::Result<(), StorageError> {
        todo!()
    }

    async fn save_accounts(&self, _accounts: Vec<Account>) -> anyhow::Result<()> {
        todo!()
    }

    async fn reset_at(&self, _number: BlockNumber) -> anyhow::Result<()> {
        todo!()
    }

    async fn read_slots_sample(&self, _start: BlockNumber, _end: BlockNumber, _max_samples: u64, _seed: u64) -> anyhow::Result<Vec<SlotSample>> {
        todo!()
    }
}
