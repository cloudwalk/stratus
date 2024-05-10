use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;

use crate::log_and_err;

pub struct IpcClient<S>
where
    S: AsyncWrite + AsyncRead + Unpin,
{
    conn: Box<S>,
}

impl<S> IpcClient<S>
where
    S: AsyncWrite + AsyncRead + Unpin,
{
    pub fn new(conn: S) -> Self {
        Self { conn: Box::new(conn) }
    }

    pub async fn write<T>(&mut self, payload: T) -> anyhow::Result<()>
    where
        T: serde::Serialize,
    {
        tracing::debug!("writing ipc payload");

        let payload = serde_json::to_vec(&payload).unwrap();

        if let Err(e) = self.conn.write_u32(payload.len() as u32).await {
            return log_and_err!(reason = e, "failed to write ipc payload length");
        };
        if let Err(e) = self.conn.write_all(&payload).await {
            return log_and_err!(reason = e, "failed to write ipc payload");
        }
        Ok(())
    }

    pub async fn read<T>(&mut self) -> anyhow::Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        tracing::debug!("reading ipc payload length");
        let payload_len = match self.conn.read_u32().await {
            Ok(len) => len,
            Err(e) => return log_and_err!(reason = e, "failed to read ipc payload length"),
        };

        tracing::debug!(bytes = &payload_len, "reading ipc payload");
        let mut payload = vec![0; payload_len as usize];
        if let Err(e) = self.conn.read_exact(&mut payload).await {
            return log_and_err!(reason = e, "failed to read ipc payload");
        }

        match serde_json::from_slice(&payload) {
            Ok(parsed) => Ok(parsed),
            Err(e) => {
                let payload = String::from_utf8_lossy(&payload);
                log_and_err!(reason = e, payload = payload, "failed to parse ipc payload")
            }
        }
    }
}
