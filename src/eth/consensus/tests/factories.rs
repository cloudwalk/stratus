use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use ethereum_types::Bloom;
use ethereum_types::H160;
use ethereum_types::H256;
use rand::Rng;
use tokio::sync::Mutex;

use crate::eth::consensus::append_entry::AppendBlockCommitResponse;
use crate::eth::consensus::append_entry::AppendTransactionExecutionsResponse;
use crate::eth::consensus::append_entry::BlockEntry;
use crate::eth::consensus::append_entry::Log;
use crate::eth::consensus::append_entry::RequestVoteResponse;
use crate::eth::consensus::append_entry::TransactionExecutionEntry;
use crate::eth::consensus::log_entry::LogEntry;
use crate::eth::consensus::Consensus;
use crate::eth::consensus::LogEntryData;
use crate::eth::consensus::Peer;
use crate::eth::consensus::PeerAddress;
use crate::eth::consensus::Role;
use crate::eth::storage::StratusStorage;

static GLOBAL_COUNTER: AtomicUsize = AtomicUsize::new(0);

pub fn zero_global_counter() {
    GLOBAL_COUNTER.store(0, Ordering::SeqCst);
}

pub fn create_mock_block_entry(transaction_hashes: Vec<Vec<u8>>, deterministic_transaction_root: Option<Hash>) -> BlockEntry {
    let transactions_root = match deterministic_transaction_root {
        Some(hash) => hash.as_fixed_bytes().to_vec(),
        None => H256::random().as_bytes().to_vec(),
    };

    let number = GLOBAL_COUNTER.fetch_add(1, Ordering::SeqCst);

    BlockEntry {
        number: number as u64,
        hash: H256::random().as_bytes().to_vec(),
        parent_hash: H256::random().as_bytes().to_vec(),
        uncle_hash: H256::random().as_bytes().to_vec(),
        transactions_root,
        state_root: H256::random().as_bytes().to_vec(),
        receipts_root: H256::random().as_bytes().to_vec(),
        miner: H160::random().as_bytes().to_vec(),
        author: H160::random().as_bytes().to_vec(),
        extra_data: vec![rand::thread_rng().gen()],
        size: rand::thread_rng().gen(),
        gas_limit: rand::thread_rng().gen(),
        gas_used: rand::thread_rng().gen(),
        timestamp: rand::thread_rng().gen(),
        bloom: Bloom::random().as_bytes().to_vec(),
        transaction_hashes,
    }
}

pub fn create_mock_transaction_execution_entry(deterministic_hash: Option<Hash>) -> TransactionExecutionEntry {
    let hash = match deterministic_hash {
        Some(hash) => hash.as_fixed_bytes().to_vec(),
        None => H256::random().as_bytes().to_vec(),
    };

    TransactionExecutionEntry {
        hash,
        nonce: rand::thread_rng().gen(),
        value: vec![rand::thread_rng().gen()],
        gas_price: vec![rand::thread_rng().gen()],
        input: vec![rand::thread_rng().gen()],
        v: rand::thread_rng().gen(),
        r: vec![rand::thread_rng().gen()],
        s: vec![rand::thread_rng().gen()],
        chain_id: Some(rand::thread_rng().gen()),
        result: "success".to_string(),
        output: vec![rand::thread_rng().gen()],
        from: H160::random().to_fixed_bytes().to_vec(),
        to: Some(H160::random().to_fixed_bytes().to_vec()),
        logs: vec![Log {
            address: H160::random().as_bytes().to_vec(),
            topics: vec![H256::random().as_bytes().to_vec()],
            data: vec![rand::thread_rng().gen()],
        }],
        gas: vec![rand::thread_rng().gen()],
        tx_type: Some(rand::thread_rng().gen()),
        signer: H160::random().to_fixed_bytes().to_vec(),
        gas_limit: vec![rand::thread_rng().gen()],
        deployed_contract_address: Some(H160::random().to_fixed_bytes().to_vec()),
        block_timestamp: rand::thread_rng().gen(),
    }
}

pub fn create_mock_log_entry_data_block() -> LogEntryData {
    LogEntryData::BlockEntry(create_mock_block_entry(vec![], None))
}

pub fn create_mock_log_entry_data_transactions() -> LogEntryData {
    LogEntryData::TransactionExecutionEntries(vec![
        create_mock_transaction_execution_entry(None),
        create_mock_transaction_execution_entry(None),
    ])
}

pub fn create_mock_log_entry(index: u64, term: u64, data: LogEntryData) -> LogEntry {
    LogEntry { index, term, data }
}

pub fn create_mock_consensus() -> Arc<Consensus> {
    let (storage, _tmpdir) = StratusStorage::mock_new_rocksdb();
    storage.set_pending_block_number_as_next_if_not_set().unwrap();
    let (_log_entries_storage, tmpdir_log_entries) = StratusStorage::mock_new_rocksdb();
    let direct_peers = Vec::new();
    let importer_config = None;
    let jsonrpc_address = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 0);
    let grpc_address = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 0);

    let tmpdir_log_entries_path = tmpdir_log_entries.path().to_str().map(|s| s.to_owned());
    let storage = Arc::new(storage);

    let miner = Miner::new(
        Arc::clone(&storage),
        crate::eth::miner::MinerMode::External, //XXX this should be passed as an argument, leaders start with interval, followers with the soon to be implemented follower mode
        None,
    );

    Consensus::new(
        Arc::clone(&storage),
        miner.into(),
        tmpdir_log_entries_path,
        direct_peers,
        importer_config,
        jsonrpc_address,
        grpc_address,
    )
}

use tonic::service::Interceptor;

use super::Hash;
use super::Miner;

// Define a simple interceptor that does nothing
#[allow(dead_code)] // HACK to avoid unused code warning
struct MockInterceptor;
impl Interceptor for MockInterceptor {
    fn call(&mut self, req: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
        Ok(req)
    }
}

fn create_mock_leader_peer(consensus: Arc<Consensus>) -> (PeerAddress, Peer) {
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

pub async fn create_follower_consensus_with_leader(term: Option<u64>) -> Arc<Consensus> {
    let consensus = create_mock_consensus();
    consensus.set_role(Role::Follower);

    if let Some(term) = term {
        consensus.current_term.store(term, Ordering::SeqCst);
    }

    let (leader_address, leader_peer) = create_mock_leader_peer(Arc::clone(&consensus));

    let mut peers = consensus.peers.write().await;
    peers.insert(leader_address, (leader_peer, tokio::spawn(async {})));

    zero_global_counter();
    Arc::clone(&consensus)
}

pub fn create_leader_consensus() -> Arc<Consensus> {
    let consensus = create_mock_consensus();
    consensus.set_role(Role::Leader);
    zero_global_counter();
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

    #[allow(clippy::unused_async)]
    pub async fn append_transaction_executions(
        &mut self,
        _request: impl tonic::IntoRequest<super::AppendTransactionExecutionsRequest>,
    ) -> std::result::Result<tonic::Response<AppendTransactionExecutionsResponse>, tonic::Status> {
        Ok(tonic::Response::new(AppendTransactionExecutionsResponse {
            status: super::StatusCode::AppendSuccess as i32,
            message: "Mock response".to_string(),
            match_log_index: 0,
            last_log_index: 0,
            last_log_term: 0,
        }))
    }

    #[allow(clippy::unused_async)]
    pub async fn append_block_commit(
        &mut self,
        _request: impl tonic::IntoRequest<super::AppendBlockCommitRequest>,
    ) -> std::result::Result<tonic::Response<AppendBlockCommitResponse>, tonic::Status> {
        Ok(tonic::Response::new(AppendBlockCommitResponse {
            status: super::StatusCode::AppendSuccess as i32,
            message: "Mock response".to_string(),
            match_log_index: 0,
            last_log_index: 0,
            last_log_term: 0,
        }))
    }

    #[allow(clippy::unused_async)]
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
