use std::sync::atomic::Ordering;

use anyhow::Result;

use super::Block;
use super::Consensus;
use super::LogEntryData;

#[derive(Debug)]
enum AppendRequest {
    BlockCommitRequest(tonic::Request<AppendBlockCommitRequest>),
    TransactionExecutionsRequest(tonic::Request<AppendTransactionExecutionsRequest>),
}

#[derive(Debug)]
enum AppendResponse {
    BlockCommitResponse(tonic::Response<AppendBlockCommitResponse>),
    TransactionExecutionsResponse(tonic::Response<AppendTransactionExecutionsResponse>),
}

pub async fn save_and_handle_log_entry(consensus: &Consensus, log_entry_data: LogEntryData) -> Result<()> {
    let last_index = consensus.log_entries_storage.get_last_index().unwrap_or(0);
    tracing::debug!(last_index, "Last index fetched");

    let current_term = consensus.current_term.load(Ordering::SeqCst);
    tracing::debug!(current_term, "Current term loaded");

    consensus
        .log_entries_storage
        .save_log_entry(last_index + 1, current_term, log_entry_data.clone(), true)?;

    consensus.prev_log_index.store(last_index + 1, Ordering::SeqCst);
    tracing::info!("Entry saved successfully");

    match log_entry_data {
        LogEntryData::BlockEntry(_) =>
            if consensus.broadcast_sender.send(log_entry_data).is_err() {
                tracing::debug!("Failed to broadcast block");
            },
        LogEntryData::TransactionExecutionEntries(_) => {
            let peers = consensus.peers.read().await;
            for (_, (peer, _)) in peers.iter() {
                let mut peer_clone = peer.clone();
                let _ = consensus.append_entry_to_peer(&mut peer_clone, &log_entry_data).await;
            }
        }
    }

    Ok(())
}

pub async fn handle_block_entry(consensus: &Consensus, block: Block) {
    if Consensus::is_leader() {
        tracing::info!(number = block.header.number.as_u64(), "Leader received block to send to followers");

        let transaction_hashes: Vec<Vec<u8>> = block.transactions.iter().map(|tx| tx.input.hash.as_fixed_bytes().to_vec()).collect();
        let block_entry = LogEntryData::BlockEntry(block.header.to_append_entry_block_header(transaction_hashes));

        if let Err(e) = save_and_handle_log_entry(consensus, block_entry).await {
            tracing::error!("Failed to save block entry: {:?}", e);
        }
    }
}

pub async fn handle_transaction_executions(consensus: &Consensus) {
    if Consensus::is_leader() {
        let mut queue = consensus.transaction_execution_queue.lock().await;
        let executions = queue.drain(..).collect::<Vec<_>>();
        drop(queue);

        tracing::debug!(executions_len = executions.len(), "Processing transaction executions");

        if let Err(e) = save_and_handle_log_entry(consensus, LogEntryData::TransactionExecutionEntries(executions)).await {
            tracing::error!("Failed to save transaction execution entry: {:?}", e);
        }
    }
}

async fn handle_peer_propagation(mut peer: Peer, consensus: Arc<Consensus>) {
    const TASK_NAME: &str = "consensus::propagate";

    let mut log_entry_queue: Vec<LogEntryData> = Vec::new();
    loop {
        if GlobalState::is_shutdown_warn(TASK_NAME) {
            return;
        };

        let receive_log_entry_from_peer = async {
            let message = peer.receiver.lock().await.recv().await;
            match message {
                Ok(log_entry) => {
                    log_entry_queue.push(log_entry);
                }
                Err(e) => {
                    tracing::warn!("Error receiving log entry for peer {:?}: {:?}", peer.client, e);
                }
            }
        };

        tokio::select! {
            biased;
            _ = GlobalState::wait_shutdown_warn(TASK_NAME) => return,
            _ = receive_log_entry_from_peer => {},
        };

        while let Some(log_entry) = log_entry_queue.first() {
            match log_entry {
                LogEntryData::BlockEntry(_block) => {
                    tracing::info!(
                        "sending block to peer: peer.match_index: {:?}, peer.next_index: {:?}",
                        peer.match_index,
                        peer.next_index
                    );
                    match consensus.append_entry_to_peer(&mut peer, log_entry).await {
                        Ok(_) => {
                            log_entry_queue.remove(0);
                            tracing::info!("successfully appended block to peer: {:?}", peer.client);
                        }
                        Err(e) => {
                            tracing::warn!("failed to append block to peer {:?}: {:?}", peer.client, e);
                            traced_sleep(RETRY_DELAY, SleepReason::RetryBackoff).await;
                        }
                    }
                }
                LogEntryData::TransactionExecutionEntries(transaction_executions) => {
                    tracing::info!("adding transaction executions to queue");
                    consensus.transaction_execution_queue.lock().await.extend(transaction_executions.clone());
                    log_entry_queue.remove(0);
                }
            }
        }
    }
}

async fn append_entry_to_peer(&self, peer: &mut Peer, entry_data: &LogEntryData) -> Result<(), anyhow::Error> {
    if !Self::is_leader() {
        tracing::error!("append_entry_to_peer called on non-leader node");
        return Err(anyhow!("append_entry_to_peer called on non-leader node"));
    }

    let current_term = self.current_term.load(Ordering::SeqCst);
    let target_index = self.log_entries_storage.get_last_index().unwrap_or(0) + 1;
    let mut next_index = peer.next_index;

    // Special case when follower has no entries and its next_index is defaulted to leader's last index + 1.
    // This exists to handle the case of a follower with an empty log
    if next_index == 0 {
        next_index = self.log_entries_storage.get_last_index().unwrap_or(0);
    }

    while next_index < target_index {
        let prev_log_index = next_index.saturating_sub(1);
        let prev_log_term = if prev_log_index == 0 {
            0
        } else {
            match self.log_entries_storage.get_entry(prev_log_index) {
                Ok(Some(entry)) => entry.term,
                Ok(None) => {
                    tracing::warn!("no log entry found at index {}", prev_log_index);
                    0
                }
                Err(e) => {
                    tracing::error!("error getting log entry at index {}: {:?}", prev_log_index, e);
                    return Err(anyhow!("error getting log entry"));
                }
            }
        };

        let entry_to_send = if next_index < target_index {
            match self.log_entries_storage.get_entry(next_index) {
                Ok(Some(entry)) => entry.data.clone(),
                Ok(None) => {
                    tracing::error!("no log entry found at index {}", next_index);
                    return Err(anyhow!("missing log entry"));
                }
                Err(e) => {
                    tracing::error!("error getting log entry at index {}: {:?}", next_index, e);
                    return Err(anyhow!("error getting log entry"));
                }
            }
        } else {
            entry_data.clone()
        };

        tracing::info!(
            "appending entry to peer: current_term: {}, prev_log_term: {}, prev_log_index: {}, target_index: {}, next_index: {}",
            current_term,
            prev_log_term,
            prev_log_index,
            target_index,
            next_index
        );

        let response = self
            .send_append_entry_request(peer, current_term, prev_log_index, prev_log_term, &entry_to_send)
            .await?;

        let (response_status, _response_message, response_match_log_index, response_last_log_index, _response_last_log_term) = match response {
            AppendResponse::BlockCommitResponse(res) => {
                let inner: AppendBlockCommitResponse = res.into_inner();
                (inner.status, inner.message, inner.match_log_index, inner.last_log_index, inner.last_log_term)
            }
            AppendResponse::TransactionExecutionsResponse(res) => {
                let inner: AppendTransactionExecutionsResponse = res.into_inner();
                (inner.status, inner.message, inner.match_log_index, inner.last_log_index, inner.last_log_term)
            }
        };

        match StatusCode::try_from(response_status) {
            Ok(StatusCode::AppendSuccess) => {
                peer.match_index = response_match_log_index;
                peer.next_index = response_match_log_index + 1;
                tracing::info!(
                    "successfully appended entry to peer: match_index: {}, next_index: {}",
                    peer.match_index,
                    peer.next_index
                );
                next_index += 1;
            }
            Ok(StatusCode::LogMismatch | StatusCode::TermMismatch) => {
                tracing::warn!(
                    "failed to append entry due to log mismatch or term mismatch. Peer last log index: {}",
                    response_last_log_index
                );
                next_index = response_last_log_index + 1;
            }
            _ => {
                tracing::error!("failed to append entry due to unexpected status code");
                return Err(anyhow!("failed to append entry due to unexpected status code"));
            }
        }
    }
    Ok(())
}

async fn send_append_entry_request(
    &self,
    peer: &mut Peer,
    current_term: u64,
    prev_log_index: u64,
    prev_log_term: u64,
    entry_data: &LogEntryData,
) -> Result<AppendResponse, anyhow::Error> {
    let request = match entry_data {
        LogEntryData::BlockEntry(block_entry) => AppendRequest::BlockCommitRequest(Request::new(AppendBlockCommitRequest {
            term: current_term,
            prev_log_index,
            prev_log_term,
            block_entry: Some(block_entry.clone()),
            leader_id: self.my_address.to_string(),
        })),
        LogEntryData::TransactionExecutionEntries(executions) =>
            AppendRequest::TransactionExecutionsRequest(Request::new(AppendTransactionExecutionsRequest {
                term: current_term,
                prev_log_index,
                prev_log_term,
                executions: executions.clone(),
                leader_id: self.my_address.to_string(),
            })),
    };

    tracing::info!(
        "sending append request. term: {}, prev_log_index: {}, prev_log_term: {}",
        current_term,
        prev_log_index,
        prev_log_term,
    );

    let response = match request {
        AppendRequest::BlockCommitRequest(request) => peer
            .client
            .append_block_commit(request)
            .await
            .map(AppendResponse::BlockCommitResponse)
            .map_err(|e| anyhow::anyhow!("failed to append block commit: {}", e)),
        AppendRequest::TransactionExecutionsRequest(request) => peer
            .client
            .append_transaction_executions(request)
            .await
            .map(AppendResponse::TransactionExecutionsResponse)
            .map_err(|e| anyhow::anyhow!("failed to append transaction executions: {}", e)),
    }?;

    Ok(response)
}
