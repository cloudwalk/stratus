use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::sync::Arc;

use ethereum_types::H160;
use ethereum_types::H256;
use rand::Rng;
use tokio::sync::broadcast;
use tokio::sync::Mutex;

use crate::eth::consensus::append_entry::AppendBlockCommitResponse;
use crate::eth::consensus::append_entry::AppendTransactionExecutionsResponse;
use crate::eth::consensus::append_entry::Log;
use crate::eth::consensus::append_entry::RequestVoteResponse;
use crate::eth::consensus::append_entry::TransactionExecutionEntry;
use crate::eth::consensus::log_entry::LogEntry;
use crate::eth::consensus::BlockEntry;
use crate::eth::consensus::Consensus;
use crate::eth::consensus::LogEntryData;
use crate::eth::consensus::Peer;
use crate::eth::consensus::PeerAddress;
use crate::eth::consensus::Role;
use crate::eth::storage::StratusStorage;

pub fn create_mock_block_entry(transaction_hashes: Vec<Vec<u8>>) -> BlockEntry {
    BlockEntry {
        number: rand::thread_rng().gen(),
        hash: H256::random().as_bytes().to_vec(),
        parent_hash: H256::random().as_bytes().to_vec(),
        uncle_hash: H256::random().as_bytes().to_vec(),
        transactions_root: H256::random().as_bytes().to_vec(),
        state_root: H256::random().as_bytes().to_vec(),
        receipts_root: H256::random().as_bytes().to_vec(),
        miner: H160::random().as_bytes().to_vec(),
        author: H160::random().as_bytes().to_vec(),
        extra_data: vec![rand::thread_rng().gen()],
        size: rand::thread_rng().gen(),
        gas_limit: rand::thread_rng().gen(),
        gas_used: rand::thread_rng().gen(),
        timestamp: rand::thread_rng().gen(),
        bloom: H256::random().as_bytes().to_vec(),
        transaction_hashes,
    }
}

pub fn create_mock_transaction_execution_entry() -> TransactionExecutionEntry {
    TransactionExecutionEntry {
        hash: H256::random().as_bytes().to_vec(),
        nonce: rand::thread_rng().gen(),
        value: vec![rand::thread_rng().gen()],
        gas_price: vec![rand::thread_rng().gen()],
        input: vec![rand::thread_rng().gen()],
        v: rand::thread_rng().gen(),
        r: vec![rand::thread_rng().gen()],
        s: vec![rand::thread_rng().gen()],
        chain_id: Some(rand::thread_rng().gen()),
        result: "Success".to_string(),
        output: vec![rand::thread_rng().gen()],
        from: H160::random().as_bytes().to_vec(),
        to: Some(H160::random().as_bytes().to_vec()),
        block_number: rand::thread_rng().gen(),
        transaction_index: rand::thread_rng().gen(),
        logs: vec![Log {
            address: H160::random().as_bytes().to_vec(),
            topics: vec![H256::random().as_bytes().to_vec()],
            data: vec![rand::thread_rng().gen()],
            log_index: rand::thread_rng().gen(),
        }],
        gas: vec![rand::thread_rng().gen()],
        receipt_cumulative_gas_used: vec![rand::thread_rng().gen()],
        receipt_gas_used: vec![rand::thread_rng().gen()],
        receipt_contract_address: vec![rand::thread_rng().gen()],
        receipt_status: rand::thread_rng().gen(),
        receipt_logs_bloom: vec![rand::thread_rng().gen()],
        receipt_effective_gas_price: vec![rand::thread_rng().gen()],
        tx_type: Some(rand::thread_rng().gen()),
        signer: vec![rand::thread_rng().gen()],
        gas_limit: vec![rand::thread_rng().gen()],
        receipt_applied: rand::thread_rng().gen(),
        deployed_contract_address: Some(vec![rand::thread_rng().gen()]),
    }
}

pub fn create_mock_log_entry_data_block() -> LogEntryData {
    LogEntryData::BlockEntry(create_mock_block_entry(vec![]))
}

pub fn create_mock_log_entry_data_transactions() -> LogEntryData {
    LogEntryData::TransactionExecutionEntries(vec![create_mock_transaction_execution_entry(), create_mock_transaction_execution_entry()])
}

pub fn create_mock_log_entry(index: u64, term: u64, data: LogEntryData) -> LogEntry {
    LogEntry { index, term, data }
}

pub async fn create_mock_consensus() -> Arc<Consensus> {
    let (storage, _tmpdir) = StratusStorage::mock_new_rocksdb();
    let (_log_entries_storage, tmpdir_log_entries) = StratusStorage::mock_new_rocksdb();
    let direct_peers = Vec::new();
    let importer_config = None;
    let jsonrpc_address = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 0);
    let grpc_address = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 0);
    let (tx_pending_txs, _) = broadcast::channel(10);
    let (tx_blocks, _) = broadcast::channel(10);

    let tmpdir_log_entries_path = tmpdir_log_entries.path().to_str().map(|s| s.to_owned());

    Consensus::new(
        storage.into(),
        tmpdir_log_entries_path,
        direct_peers,
        importer_config,
        jsonrpc_address,
        grpc_address,
        tx_pending_txs.subscribe(),
        tx_blocks.subscribe(),
    )
    .await
}

use tonic::service::Interceptor;

// Define a simple interceptor that does nothing
struct MockInterceptor;
impl Interceptor for MockInterceptor {
    fn call(&mut self, req: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
        Ok(req)
    }
}

async fn create_mock_leader_peer(consensus: Arc<Consensus>) -> (PeerAddress, Peer) {
    let leader_address = PeerAddress::from_string("http://127.0.0.1:3000;3777".to_string()).unwrap();
    let client = MockAppendEntryServiceClient::new();
    let leader_peer = Peer {
        client,
        match_index: 0,
        next_index: 0,
        role: Role::Leader,
        receiver: Arc::new(Mutex::new(consensus.broadcast_sender.subscribe())),
    };
    (leader_address, leader_peer)
}

pub async fn create_follower_consensus_with_leader() -> Arc<Consensus> {
    let consensus = create_mock_consensus().await;
    consensus.set_role(Role::Follower);

    let (leader_address, leader_peer) = create_mock_leader_peer(Arc::clone(&consensus)).await;

    let mut peers = consensus.peers.write().await;
    peers.insert(leader_address, (leader_peer, tokio::spawn(async {})));

    Arc::clone(&consensus)
}

pub async fn create_leader_consensus() -> Arc<Consensus> {
    let consensus = create_mock_consensus().await;
    consensus.set_role(Role::Leader);
    consensus
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub struct MockAppendEntryServiceClient;

#[cfg(test)]
impl MockAppendEntryServiceClient {
    pub fn new() -> Self {
        MockAppendEntryServiceClient
    }

    pub async fn append_transaction_executions(
        &mut self,
        _request: impl tonic::IntoRequest<super::AppendTransactionExecutionsRequest>,
    ) -> std::result::Result<tonic::Response<AppendTransactionExecutionsResponse>, tonic::Status> {
        Ok(tonic::Response::new(AppendTransactionExecutionsResponse {
            status: super::StatusCode::AppendSuccess as i32,
            message: "Mock response".to_string(),
            last_committed_block_number: 0,
        }))
    }

    pub async fn append_block_commit(
        &mut self,
        _request: impl tonic::IntoRequest<super::AppendBlockCommitRequest>,
    ) -> std::result::Result<tonic::Response<AppendBlockCommitResponse>, tonic::Status> {
        Ok(tonic::Response::new(AppendBlockCommitResponse {
            status: super::StatusCode::AppendSuccess as i32,
            message: "Mock response".to_string(),
            last_committed_block_number: 0,
        }))
    }

    pub async fn request_vote(
        &mut self,
        _request: impl tonic::IntoRequest<super::RequestVoteRequest>,
    ) -> std::result::Result<tonic::Response<RequestVoteResponse>, tonic::Status> {
        Ok(tonic::Response::new(RequestVoteResponse {
            term: 0,
            vote_granted: true,
            message: "Mock response".to_string(),
        }))
    }
}
