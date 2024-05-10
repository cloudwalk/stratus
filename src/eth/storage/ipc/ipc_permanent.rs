use std::collections::HashMap;

use anyhow::anyhow;
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
use crate::eth::storage::PermanentStorageIpcRequest as Req;
use crate::eth::storage::PermanentStorageIpcResponse as Resp;
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

    async fn request(&self, request: Req) -> anyhow::Result<Resp> {
        let mut client = self.client.lock().await;

        // request / response
        client.write(request).await?;
        let response = client.read::<Resp>().await?;

        // handle response error
        if let Resp::Error(e) = response {
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

    async fn set_mined_block_number(&self, number: BlockNumber) -> anyhow::Result<()> {
        let req = Req::SetMinedBlockNumber(number);
        match self.request(req).await? {
            Resp::SetMinedBlockNumber(_) => Ok(()),
            resp => return log_and_err!(payload = resp, "unexpected response"),
        }
    }

    async fn read_mined_block_number(&self) -> anyhow::Result<BlockNumber> {
        let req = Req::ReadMinedBlockNumber;
        match self.request(req).await? {
            Resp::ReadMinedBlockNumber(number) => Ok(number),
            resp => return log_and_err!(payload = resp, "unexpected response"),
        }
    }

    async fn increment_block_number(&self) -> anyhow::Result<BlockNumber> {
        todo!()
    }

    async fn read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Account>> {
        let req = Req::ReadAccount(*address, *point_in_time);
        match self.request(req).await? {
            Resp::ReadAccount(account) => Ok(account),
            resp => return log_and_err!(payload = resp, "unexpected response"),
        }
    }

    async fn read_slot(&self, address: &Address, index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>> {
        let req = Req::ReadSlot(*address, *index, *point_in_time);
        match self.request(req).await? {
            Resp::ReadSlot(slot) => Ok(slot),
            resp => return log_and_err!(payload = resp, "unexpected response"),
        }
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
        let req = Req::ReadBlock(selection.clone());
        match self.request(req).await? {
            Resp::ReadBlock(block) => Ok(block),
            resp => return log_and_err!(payload = resp, "unexpected response"),
        }
    }

    async fn read_mined_transaction(&self, _hash: &Hash) -> anyhow::Result<Option<TransactionMined>> {
        todo!()
    }

    async fn read_logs(&self, _filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        todo!()
    }

    async fn save_block(&self, block: Block) -> anyhow::Result<(), StorageError> {
        let req = Req::SaveBlock(block);
        match self.request(req).await? {
            Resp::SaveBlock(None) => Ok(()),
            Resp::SaveBlock(Some(conflicts)) => Err(StorageError::Conflict(conflicts)),
            resp => Err(StorageError::Generic(anyhow!(format!("unexpected response | payload={:?}", resp)))),
        }
    }

    async fn save_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<()> {
        let req = Req::SaveAccounts(accounts);
        match self.request(req).await? {
            Resp::SaveAccounts(_) => Ok(()),
            resp => return log_and_err!(payload = resp, "unexpected response"),
        }
    }

    async fn reset_at(&self, number: BlockNumber) -> anyhow::Result<()> {
        let req = Req::ResetAt(number);
        match self.request(req).await? {
            Resp::ResetAt(_) => Ok(()),
            resp => return log_and_err!(payload = resp, "unexpected response"),
        }
    }

    async fn read_slots_sample(&self, start: BlockNumber, end: BlockNumber, max_samples: u64, seed: u64) -> anyhow::Result<Vec<SlotSample>> {
        let req = Req::ReadSlotsSample(start, end, max_samples, seed);
        match self.request(req).await? {
            Resp::ReadSlotsSample(samples) => Ok(samples),
            resp => return log_and_err!(payload = resp, "unexpected response"),
        }
    }
}
