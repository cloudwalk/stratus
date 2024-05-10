use std::fs;
use std::sync::Arc;

use futures::StreamExt;
use parity_tokio_ipc::Endpoint;
use stratus::config::IpcRocksConfig;
use stratus::eth::storage::PermanentStorage;
use stratus::eth::storage::PermanentStorageIpcRequest as Req;
use stratus::eth::storage::PermanentStorageIpcResponse as Resp;
use stratus::eth::storage::RocksPermanentStorage;
use stratus::eth::storage::StorageError;
use stratus::infra::IpcClient;
use stratus::GlobalServices;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;

fn main() -> anyhow::Result<()> {
    let global_services = GlobalServices::<IpcRocksConfig>::init();
    global_services.runtime.block_on(run(global_services.config))
}

async fn run(_: IpcRocksConfig) -> anyhow::Result<()> {
    let socket = "./data/perm-storage.sock".to_string();
    tracing::info!(%socket, "starting rocksdb ipc server");

    // init rocksdb storage
    let rocks = Arc::new(RocksPermanentStorage::new().await?);

    // init socket endpoint
    let _ = fs::remove_file(&socket);
    let socket = Endpoint::new(socket).incoming()?;

    socket
        .for_each(|connection| async {
            let rocks = Arc::clone(&rocks);
            match connection {
                Ok(stream) => {
                    tracing::info!("client connected");
                    tokio::spawn(handle_client(stream, rocks));
                }
                Err(e) => {
                    tracing::error!(reason = ?e, "failed to accept accepting connection");
                }
            }
        })
        .await;

    Ok(())
}

async fn handle_client<T>(stream: T, rocks: Arc<RocksPermanentStorage>) -> anyhow::Result<()>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    let mut client = IpcClient::new(stream);

    loop {
        // read request
        let request = match client.read::<Req>().await {
            Ok(request) => request,
            Err(e) => {
                client.write(Resp::Error(e.to_string())).await?;
                continue;
            }
        };

        // handle request
        let response = match request {
            Req::ReadAccount(address, point_in_time) => rocks.read_account(&address, &point_in_time).await.map(|account| Resp::ReadAccount(account)),
            Req::ReadBlock(selection) => rocks.read_block(&selection).await.map(|block| Resp::ReadBlock(block)),
            Req::ReadMinedBlockNumber => rocks.read_mined_block_number().await.map(|number| Resp::ReadMinedBlockNumber(number)),
            Req::ReadSlot(address, index, point_in_time) => rocks.read_slot(&address, &index, &point_in_time).await.map(|number| Resp::ReadSlot(number)),
            Req::SaveAccounts(accounts) => rocks.save_accounts(accounts).await.map(|_| Resp::SaveAccounts(())),
            Req::SaveBlock(accounts) => match rocks.save_block(accounts).await {
                Ok(_) => Ok(Resp::SaveBlock(None)),
                Err(StorageError::Conflict(conflicts)) => Ok(Resp::SaveBlock(Some(conflicts))),
                Err(StorageError::Generic(e)) => Err(e),
            },
            Req::SetMinedBlockNumber(number) => rocks.set_mined_block_number(number).await.map(|_| Resp::SetMinedBlockNumber(())),
            req => {
                tracing::error!(?req, "unhandle request");
                Ok(Resp::Error("unhandled request".to_string()))
            }
        };

        // send response
        let response = match response {
            Ok(response) => response,
            Err(e) => Resp::Error(e.to_string()),
        };
        client.write(response).await?;
    }
}
