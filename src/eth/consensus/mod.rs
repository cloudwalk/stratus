pub mod forward_to;

use std::collections::HashMap;
use std::env;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
#[cfg(feature = "kubernetes")]
use k8s_openapi::api::core::v1::Pod;
#[cfg(feature = "kubernetes")]
use kube::api::Api;
#[cfg(feature = "kubernetes")]
use kube::api::ListParams;
#[cfg(feature = "kubernetes")]
use kube::Client;
use tokio::sync::broadcast;
use tokio::sync::mpsc::Sender;
use tokio::sync::mpsc::{self};
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tonic::transport::Channel;
use tonic::transport::Server;
use tonic::Request;
use tonic::Response;
use tonic::Status;

use crate::channel_read;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::storage::StratusStorage;
use crate::ext::named_spawn;
use crate::infra::BlockchainClient;

pub mod append_entry {
    tonic::include_proto!("append_entry");
}

use append_entry::append_entry_service_client::AppendEntryServiceClient;
use append_entry::append_entry_service_server::AppendEntryService;
use append_entry::append_entry_service_server::AppendEntryServiceServer;
use append_entry::RequestVoteRequest;
use append_entry::RequestVoteResponse;
use append_entry::AppendBlockCommitRequest;
use append_entry::AppendBlockCommitResponse;
use append_entry::AppendTransactionExecutionsRequest;
use append_entry::AppendTransactionExecutionsResponse;
use append_entry::BlockHeader;
use append_entry::StatusCode;

use super::primitives::TransactionInput;
use crate::config::RunWithImporterConfig;
use crate::eth::primitives::Block;
#[cfg(feature = "metrics")]
use crate::infra::metrics;

const RETRY_DELAY: Duration = Duration::from_millis(10);

#[derive(Clone, Debug, PartialEq)]
enum Role {
    Leader,
    Follower,
    Candidate,
}

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
struct PeerAddress(String);

#[derive(Clone)]
struct Peer {
    client: AppendEntryServiceClient<Channel>,
    last_heartbeat: std::time::Instant, //TODO implement metrics for this
    match_index: u64,
    next_index: u64,
    role: Role,
    term: u64,
    receiver: Arc<Mutex<broadcast::Receiver<Block>>>,
}

type PeerTuple = (Peer, JoinHandle<()>);

pub struct Consensus {
    pub sender: Sender<Block>,                      //receives blocks
    broadcast_sender: broadcast::Sender<Block>,     //propagates the blocks
    importer_config: Option<RunWithImporterConfig>, //HACK this is used with sync online only
    storage: Arc<StratusStorage>,
    peers: Arc<RwLock<HashMap<PeerAddress, PeerTuple>>>,
    direct_peers: Vec<String>,
    voted_for: Mutex<Option<PeerAddress>>,
    current_term: AtomicU64,
    last_arrived_block_number: AtomicU64, //TODO use a true index for both executions and blocks, currently we use something like Bully algorithm so block number is fine
    role: RwLock<Role>,
    heartbeat_timeout: Duration,
    election_timeout: Duration,
}

impl Consensus {
    pub async fn new(storage: Arc<StratusStorage>, direct_peers: Vec<String>, importer_config: Option<RunWithImporterConfig>) -> Arc<Self> {
        let (sender, receiver) = mpsc::channel::<Block>(32);
        let receiver = Arc::new(Mutex::new(receiver));
        let (broadcast_sender, _) = broadcast::channel(32);
        let last_arrived_block_number = AtomicU64::new(storage.read_mined_block_number().await.unwrap_or(BlockNumber::from(0)).into());
        let peers = Arc::new(RwLock::new(HashMap::new()));

        let consensus = Self {
            sender,
            broadcast_sender,
            storage,
            peers,
            direct_peers,
            current_term: AtomicU64::new(0),
            voted_for: Mutex::new(None),
            last_arrived_block_number,
            importer_config,
            role: RwLock::new(Role::Follower),
            heartbeat_timeout: Duration::from_millis(150), // Adjust as needed
            election_timeout: Duration::from_millis(300), // Adjust as needed
        };
        let consensus = Arc::new(consensus);

        Self::initialize_periodic_peer_discovery(Arc::clone(&consensus));
        Self::initialize_append_entries_channel(Arc::clone(&consensus), Arc::clone(&receiver));
        Self::initialize_server(Arc::clone(&consensus));
        Self::initialize_heartbeat_timer(Arc::clone(&consensus));

        consensus
    }

    fn initialize_heartbeat_timer(consensus: Arc<Consensus>) {
        named_spawn("consensus::heartbeat_timer", async move {
            loop {
                sleep(consensus.heartbeat_timeout).await;
                if *consensus.role.read().await != Role::Leader {
                    let mut leader_found = false;
                    {
                        let peers = consensus.peers.read().await;
                        for (_, (peer, _)) in peers.iter() {
                            if peer.last_heartbeat.elapsed() < consensus.election_timeout {
                                leader_found = true;
                                break;
                            }
                        }
                    }
                    if !leader_found {
                        consensus.start_election().await;
                    }
                }
            }
        });
    }

    async fn start_election(&self) {
        let mut term = self.current_term.fetch_add(1, Ordering::SeqCst) + 1;
        self.current_term.store(term, Ordering::SeqCst);

        *self.voted_for.lock().await = Some(PeerAddress(Self::current_node().unwrap()));

        let mut votes = 1; // Vote for self

        let peers = self.peers.read().await;
        for (peer_address, (peer, _)) in peers.iter() {
            let request = Request::new(RequestVoteRequest {
                term,
                candidate_id: Self::current_node().unwrap(),
                last_log_index: self.last_arrived_block_number.load(Ordering::SeqCst),
                last_log_term: term,
            });

            match peer.client.request_vote(request).await {
                Ok(response) => {
                    if response.into_inner().vote_granted {
                        votes += 1;
                    }
                }
                Err(_) => {
                    tracing::warn!("Failed to request vote from {:?}", peer_address);
                }
            }
        }

        if votes > peers.len() / 2 {
            self.become_leader().await;
        } else {
            *self.role.write().await = Role::Follower;
        }
    }

    async fn become_leader(&self) {
        tracing::info!("Became the leader");
        *self.role.write().await = Role::Leader;

        // Initialize leader-specific tasks such as sending appendEntries
        self.send_append_entries().await;
    }

    async fn send_append_entries(&self) {
        loop {
            if *self.role.read().await == Role::Leader {
                let peers = self.peers.read().await;
                for (_, (peer, _)) in peers.iter() {
                    let request = Request::new(AppendEntriesRequest {
                        term: self.current_term.load(Ordering::SeqCst),
                        leader_id: Self::current_node().unwrap(),
                        prev_log_index: 0, // Adjust as needed
                        prev_log_term: 0, // Adjust as needed
                        entries: vec![], // Empty for heartbeat
                        leader_commit: self.last_arrived_block_number.load(Ordering::SeqCst),
                    });
                    if let Err(e) = peer.client.append_entries(request).await {
                        tracing::warn!("Failed to send appendEntries to {:?}: {:?}", peer.client, e);
                    }
                }
                sleep(Duration::from_millis(100)).await; // Adjust as needed
            } else {
                break;
            }
        }
    }

    fn initialize_periodic_peer_discovery(consensus: Arc<Consensus>) {
        named_spawn("consensus::peer_discovery", async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            loop {
                tracing::info!("Starting periodic peer discovery...");
                Self::discover_peers(Arc::clone(&consensus)).await;
                interval.tick().await;
            }
        });
    }

    fn initialize_append_entries_channel(consensus: Arc<Consensus>, receiver: Arc<Mutex<mpsc::Receiver<Block>>>) {
        //TODO add data to consensus-log-transactions
        //TODO at the begining of temp-storage, load the consensus-log-transactions so the index becomes clear
        //TODO use gRPC instead of jsonrpc
        //FIXME for now, this has no colateral efects, but it will have in the future
        //TODO rediscover followers on comunication error
        named_spawn("consensus::sender", async move {
            loop {
                let mut receiver_lock = receiver.lock().await;
                if let Some(data) = receiver_lock.recv().await {
                    if consensus.is_leader().await {
                        tracing::info!(number = data.header.number.as_u64(), "received block to send to followers");

                        if let Err(e) = consensus.broadcast_sender.send(data) {
                            tracing::warn!("Failed to broadcast block: {:?}", e);
                        }
                    }
                }
            }
        });
    }

    fn initialize_server(consensus: Arc<Consensus>) {
        named_spawn("consensus::server", async move {
            tracing::info!("Starting append entry service at port 3777");
            let addr = "0.0.0.0:3777".parse().unwrap();

            let append_entry_service = AppendEntryServiceImpl {
                consensus: Mutex::new(consensus),
            };

            Server::builder()
                .add_service(AppendEntryServiceServer::new(append_entry_service))
                .serve(addr)
                .await
                .unwrap();
        });
    }

    //FIXME TODO automate the way we gather the leader, instead of using a env var
    pub async fn is_leader(&self) -> bool {
        *self.role.read().await == Role::Leader
    }

    pub async fn is_follower(&self) -> bool {
        *self.role.read().await == Role::Follower
    }

    pub async fn should_forward(&self) -> bool {
        tracing::info!(
            is_leader = self.is_leader().await,
            sync_online_enabled = self.importer_config.is_some(),
            "handling request forward"
        );
        if self.is_leader().await && self.importer_config.is_none() {
            return false; // the leader is on miner mode and should deal with the requests
        }
        true
    }

    pub async fn forward(&self, transaction: TransactionInput) -> anyhow::Result<Hash> {
        //TODO rename to TransactionForward
        let Some((http_url, _)) = self.get_chain_url() else {
            return Err(anyhow!("No chain url found"));
        };
        let chain = BlockchainClient::new_http(&http_url, Duration::from_secs(2)).await?;
        let forward_to = forward_to::TransactionRelayer::new(chain);
        let result = forward_to.forward(transaction).await?;
        Ok(result.tx_hash) //XXX HEX
    }

    //TODO for now the block number is the index, but it should be a separate index wiht the execution AND the block
    pub async fn should_serve(&self) -> bool {
        let last_arrived_block_number = self.last_arrived_block_number.load(Ordering::SeqCst);
        let storage_block_number: u64 = self.storage.read_mined_block_number().await.unwrap_or(BlockNumber::from(0)).into();

        tracing::info!(
            "last arrived block number: {}, storage block number: {}",
            last_arrived_block_number,
            storage_block_number
        );

        (last_arrived_block_number - 2) <= storage_block_number
    }

    fn current_node() -> Option<String> {
        let pod_name = env::var("MY_POD_NAME").ok()?;
        Some(pod_name.trim().to_string())
    }

    fn current_namespace() -> Option<String> {
        let namespace = env::var("NAMESPACE").ok()?;
        Some(namespace.trim().to_string())
    }

    // XXX this is a temporary solution to get the leader node
    // later we want the leader to GENERATE blocks
    // and even later we want this sync to be replaced by a gossip protocol or raft
    pub fn get_chain_url(&self) -> Option<(String, Option<String>)> {
        if self.is_follower() {
            if let Some(namespace) = Self::current_namespace() {
                return Some((format!("http://{}.stratus-api.{}.svc.cluster.local:3000", self.leader_name, namespace), None));
                //TODO use peer discovery to discover the leader
            }
        }

        match self.importer_config.clone() {
            Some(importer_config) => Some((importer_config.online.external_rpc, importer_config.online.external_rpc_ws)),
            None => None,
        }
    }

    #[tracing::instrument(skip_all)]
    pub async fn discover_peers(consensus: Arc<Consensus>) {
        let mut new_peers: Vec<(PeerAddress, Peer)> = Vec::new();

        #[cfg(feature = "kubernetes")]
        if let Ok(k8s_peers) = Self::discover_peers_kubernetes(Arc::clone(&consensus)).await {
            new_peers.extend(k8s_peers);
        }

        if let Ok(env_peers) = Self::discover_peers_env(&consensus.direct_peers, Arc::clone(&consensus)).await {
            new_peers.extend(env_peers);
        }

        let mut peers_lock = consensus.peers.write().await;

        for (address, new_peer) in new_peers {
            if peers_lock.contains_key(&address) {
                tracing::info!("Peer {} already exists, skipping initialization", address.0);
                continue;
            }

            let peer = Peer {
                receiver: Arc::new(Mutex::new(consensus.broadcast_sender.subscribe())),
                ..new_peer
            };

            let consensus_clone = Arc::clone(&consensus);
            let peer_clone = peer.clone();

            let handle = named_spawn("consensus::propagate", async move {
                Self::handle_peer_block_propagation(peer_clone, consensus_clone).await;
            });

            tracing::info!("Adding new peer: {}", address.0);
            peers_lock.insert(address, (peer, handle));
        }

        tracing::info!(
            peers = ?peers_lock.keys().collect::<Vec<&PeerAddress>>(),
            "Discovered peers",
        );
    }

    async fn discover_peers_env(addresses: &[String], consensus: Arc<Consensus>) -> Result<Vec<(PeerAddress, Peer)>, anyhow::Error> {
        let mut peers: Vec<(PeerAddress, Peer)> = Vec::new();

        for address in addresses {
            match AppendEntryServiceClient::connect(address.clone()).await {
                Ok(client) => {
                    let peer = Peer {
                        client,
                        last_heartbeat: std::time::Instant::now(),
                        match_index: 0,
                        next_index: 0,
                        role: Role::Follower, //FIXME it wont be always follower, we need to check the leader or candidates
                        term: 0,              // Replace with actual term
                        receiver: Arc::new(Mutex::new(consensus.broadcast_sender.subscribe())),
                    };
                    peers.push((PeerAddress(address.clone()), peer));
                    tracing::info!("Peer {} is available", address);
                }
                Err(_) => {
                    tracing::warn!("Peer {} is not available", address);
                }
            }
        }

        Ok(peers)
    }

    #[cfg(feature = "kubernetes")]
    async fn discover_peers_kubernetes(consensus: Arc<Consensus>) -> Result<Vec<(PeerAddress, Peer)>, anyhow::Error> {
        let mut peers: Vec<(PeerAddress, Peer)> = Vec::new();

        let client = Client::try_default().await?;
        let pods: Api<Pod> = Api::namespaced(client, &Self::current_namespace().unwrap_or("default".to_string()));

        let lp = ListParams::default().labels("app=stratus-api");
        let pod_list = pods.list(&lp).await?;

        for p in pod_list.items {
            if let Some(pod_name) = p.metadata.name {
                if pod_name != Self::current_node().unwrap() {
                    if let Some(namespace) = Self::current_namespace() {
                        let address = format!("http://{}.stratus-api.{}.svc.cluster.local:3777", pod_name, namespace);
                        let client = AppendEntryServiceClient::connect(address.clone()).await?;

                        let peer = Peer {
                            client,
                            last_heartbeat: std::time::Instant::now(),
                            match_index: 0,
                            next_index: 0,
                            role: Role::Follower, //FIXME it wont be always follower, we need to check the leader or candidates
                            term: 0,              // Replace with actual term
                            receiver: Arc::new(Mutex::new(consensus.broadcast_sender.subscribe())),
                        };
                        peers.push((PeerAddress(address), peer));
                    }
                }
            }
        }

        Ok(peers)
    }

    async fn handle_peer_block_propagation(mut peer: Peer, consensus: Arc<Consensus>) {
        let mut block_queue: Vec<Block> = Vec::new();
        loop {
            let mut receiver_lock = peer.receiver.lock().await;
            match receiver_lock.recv().await {
                Ok(block) => {
                    block_queue.push(block.clone());
                }
                Err(e) => {
                    tracing::warn!("Error receiving block for peer {:?}: {:?}", peer.client, e);
                }
            }
            drop(receiver_lock); // Drop the immutable borrow before making a mutable borrow
            while let Some(block) = block_queue.first() {
                match consensus.append_block_to_peer(&mut peer, block).await {
                    Ok(_) => {
                        block_queue.remove(0); // Remove the successfully sent block from the queue
                        tracing::info!("Successfully appended block to peer: {:?}", peer.client);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to append block to peer {:?}: {:?}", peer.client, e);
                        sleep(RETRY_DELAY).await;
                    }
                }
            }
        }
    }

    async fn append_block_to_peer(&self, peer: &mut Peer, block: &Block) -> Result<(), anyhow::Error> {
        let header: BlockHeader = (&block.header).into();
        let transaction_hashes = vec![]; // Replace with actual transaction hashes

        let request = Request::new(AppendBlockCommitRequest {
            term: 0,
            prev_log_index: 0,
            prev_log_term: 0,
            header: Some(header),
            transaction_hashes,
        });

        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let response = peer.client.append_block_commit(request).await?;
        let response = response.into_inner();

        #[cfg(feature = "metrics")]
        metrics::inc_append_entries(start.elapsed());

        tracing::info!(last_heartbeat = ?peer.last_heartbeat, match_index = peer.match_index, next_index = peer.next_index, role = ?peer.role, term = peer.term,  "current follower state"); //TODO also move this to metrics

        match StatusCode::try_from(response.status) {
            Ok(StatusCode::AppendSuccess) => Ok(()),
            _ => Err(anyhow!("Unexpected status code: {:?}", response.status)),
        }
    }
}

pub struct AppendEntryServiceImpl {
    consensus: Mutex<Arc<Consensus>>,
}

#[tonic::async_trait]
impl AppendEntryService for AppendEntryServiceImpl {
    async fn append_transaction_executions(
        &self,
        request: Request<AppendTransactionExecutionsRequest>,
    ) -> Result<Response<AppendTransactionExecutionsResponse>, Status> {
        let executions = request.into_inner().executions;
        //TODO Process the transaction executions here
        for execution in executions {
            println!("Received transaction execution: {:?}", execution);
        }

        Ok(Response::new(AppendTransactionExecutionsResponse {
            status: StatusCode::AppendSuccess as i32,
            message: "Transaction Executions appended successfully".into(),
            last_committed_block_number: 0,
        }))
    }

    async fn append_block_commit(&self, request: Request<AppendBlockCommitRequest>) -> Result<Response<AppendBlockCommitResponse>, Status> {
        let Some(header) = request.into_inner().header else {
            return Err(Status::invalid_argument("empty block header"));
        };

        tracing::info!(number = header.number, "appending new block");

        let consensus = self.consensus.lock().await;
        let last_last_arrived_block_number = consensus.last_arrived_block_number.load(Ordering::SeqCst);

        consensus.last_arrived_block_number.store(header.number, Ordering::SeqCst);

        tracing::info!(
            last_last_arrived_block_number = last_last_arrived_block_number,
            new_last_arrived_block_number = consensus.last_arrived_block_number.load(Ordering::SeqCst),
            "last arrived block number set",
        );

        #[cfg(feature = "metrics")]
        metrics::set_append_entries_block_number_diff(last_last_arrived_block_number - header.number);

        Ok(Response::new(AppendBlockCommitResponse {
            status: StatusCode::AppendSuccess as i32,
            message: "Block Commit appended successfully".into(),
            last_committed_block_number: consensus.last_arrived_block_number.load(Ordering::SeqCst),
        }))
    }

    async fn request_vote(
        &self,
        request: Request<RequestVoteRequest>,
    ) -> Result<Response<RequestVoteResponse>, Status> {
        let request = request.into_inner();
        let consensus = self.consensus.lock().await;
        let current_term = consensus.current_term.load(Ordering::SeqCst);

        if request.term < current_term {
            return Ok(Response::new(RequestVoteResponse {
                term: current_term,
                vote_granted: false,
            }));
        }

        if request.term > current_term {
            consensus.current_term.store(request.term, Ordering::SeqCst);
            *consensus.voted_for.lock().await = None;
            *consensus.role.write().await = Role::Follower;
        }

        let mut voted_for = consensus.voted_for.lock().await;
        if voted_for.is_none() || *voted_for == Some(PeerAddress(request.candidate_id.clone())) {
            *voted_for = Some(PeerAddress(request.candidate_id.clone()));
            return Ok(Response::new(RequestVoteResponse {
                term: request.term,
                vote_granted: true,
            }));
        }

        Ok(Response::new(RequestVoteResponse {
            term: request.term,
            vote_granted: false,
        }))
    }
}
