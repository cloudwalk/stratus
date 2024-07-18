use std::sync::atomic::Ordering;

use anyhow::Result;

use super::Block;
use super::Consensus;
use super::LogEntryData;

#[allow(dead_code)]
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

#[allow(dead_code)]
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

#[allow(dead_code)]
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
