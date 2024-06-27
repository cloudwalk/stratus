#[cfg(feature = "rocks")]
#[allow(dead_code)]
mod append_log_entries_storage;
mod discovery;
pub mod forward_to;
#[allow(dead_code)]
mod log_entry;

mod server;

use std::collections::HashMap;
#[cfg(feature = "kubernetes")]
use std::env;
use std::net::SocketAddr;
use std::net::UdpSocket;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicU8;
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
use rand::Rng;
use server::AppendEntryServiceImpl;
use tokio::sync::broadcast;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
#[cfg(feature = "kubernetes")]
use tokio::time::sleep;
use tonic::transport::Server;
use tonic::Request;

use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::storage::StratusStorage;
use crate::ext::spawn_named;
use crate::ext::traced_sleep;
use crate::ext::SleepReason;
use crate::infra::BlockchainClient;
use crate::GlobalState;

pub mod append_entry {
    tonic::include_proto!("append_entry");
}
#[allow(unused_imports)]
use append_entry::append_entry_service_client::AppendEntryServiceClient;
use append_entry::append_entry_service_server::AppendEntryService;
use append_entry::append_entry_service_server::AppendEntryServiceServer;
use append_entry::AppendBlockCommitRequest;
use append_entry::AppendTransactionExecutionsRequest;
use append_entry::BlockEntry;
use append_entry::RequestVoteRequest;
use append_entry::StatusCode;
use append_entry::TransactionExecutionEntry;

use self::log_entry::LogEntryData;
use super::primitives::TransactionExecution;
use super::primitives::TransactionInput;
use crate::config::RunWithImporterConfig;
use crate::eth::primitives::Block;
#[cfg(feature = "metrics")]
use crate::infra::metrics;

const RETRY_DELAY: Duration = Duration::from_millis(10);
const PEER_DISCOVERY_DELAY: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Role {
    Leader = 1,
    Follower = 2,
    _Candidate = 3,
}

#[derive(Clone, Debug, Default, Hash, Eq, PartialEq)]
struct PeerAddress {
    address: String,
    jsonrpc_port: u16,
    grpc_port: u16,
}

impl PeerAddress {
    fn new(address: String, jsonrpc_port: u16, grpc_port: u16) -> Self {
        PeerAddress {
            address,
            jsonrpc_port,
            grpc_port,
        }
    }

    fn full_grpc_address(&self) -> String {
        format!("{}:{}", self.address, self.grpc_port)
    }

    fn full_jsonrpc_address(&self) -> String {
        format!("http://{}:{}", self.address, self.jsonrpc_port)
    }

    fn from_string(s: String) -> Result<Self, anyhow::Error> {
        let (scheme, address_part) = if let Some(address) = s.strip_prefix("http://") {
            ("http://", address)
        } else if let Some(address) = s.strip_prefix("https://") {
            ("https://", address)
        } else {
            return Err(anyhow::anyhow!("invalid scheme"));
        };

        let parts: Vec<&str> = address_part.split(':').collect();
        if parts.len() != 2 {
            return Err(anyhow::anyhow!("invalid format"));
        }
        let address = format!("{}{}", scheme, parts[0]);
        let ports: Vec<&str> = parts[1].split(';').collect();
        if ports.len() != 2 {
            return Err(anyhow::anyhow!("invalid format for jsonrpc and grpc ports"));
        }
        let jsonrpc_port = ports[0].parse::<u16>()?;
        let grpc_port = ports[1].parse::<u16>()?;
        Ok(PeerAddress {
            address,
            jsonrpc_port,
            grpc_port,
        })
    }
}

use std::fmt;

impl fmt::Display for PeerAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{};{}", self.address, self.jsonrpc_port, self.grpc_port)
    }
}

#[cfg(test)]
use crate::eth::consensus::tests::factories::MockAppendEntryServiceClient;

#[cfg(not(test))]
type ClientType = AppendEntryServiceClient<tonic::transport::Channel>;
#[cfg(test)]
type ClientType = MockAppendEntryServiceClient;

#[derive(Clone)]
struct Peer {
    client: ClientType,
    match_index: u64,
    next_index: u64,
    role: Role,
    receiver: Arc<Mutex<broadcast::Receiver<LogEntryData>>>,
}

type PeerTuple = (Peer, JoinHandle<()>);

pub struct Consensus {
    broadcast_sender: broadcast::Sender<LogEntryData>, //propagates the blocks
    importer_config: Option<RunWithImporterConfig>,    //HACK this is used with sync online only
    storage: Arc<StratusStorage>,
    peers: Arc<RwLock<HashMap<PeerAddress, PeerTuple>>>,
    #[allow(dead_code)]
    direct_peers: Vec<String>,
    voted_for: Mutex<Option<PeerAddress>>, //essential to ensure that a server only votes once per term
    current_term: AtomicU64,
    last_arrived_block_number: AtomicU64, //FIXME this should be replaced by the index on our appendEntry log
    transaction_execution_queue: Arc<Mutex<Vec<TransactionExecutionEntry>>>,
    role: AtomicU8,
    heartbeat_timeout: Duration,
    my_address: PeerAddress,
    grpc_address: SocketAddr,
    reset_heartbeat_signal: tokio::sync::Notify,
    blockchain_client: Mutex<Option<Arc<BlockchainClient>>>,
}

impl Consensus {
    pub async fn new(
        storage: Arc<StratusStorage>,
        direct_peers: Vec<String>,
        importer_config: Option<RunWithImporterConfig>,
        jsonrpc_address: SocketAddr,
        grpc_address: SocketAddr,
        rx_pending_txs: broadcast::Receiver<TransactionExecution>,
        rx_blocks: broadcast::Receiver<Block>,
    ) -> Arc<Self> {
        let (broadcast_sender, _) = broadcast::channel(32); //TODO rename to internal_peer_broadcast_sender
        let last_arrived_block_number = AtomicU64::new(0); //we use the max value to ensure that only after receiving the first appendEntry we can start the consensus
        let peers = Arc::new(RwLock::new(HashMap::new()));
        let my_address = Self::discover_my_address(jsonrpc_address.port(), grpc_address.port());

        let consensus = Self {
            broadcast_sender,
            storage,
            peers,
            direct_peers,
            current_term: AtomicU64::new(0),
            voted_for: Mutex::new(None),
            last_arrived_block_number,
            transaction_execution_queue: Arc::new(Mutex::new(Vec::new())),
            importer_config,
            role: AtomicU8::new(Role::Follower as u8),
            heartbeat_timeout: Duration::from_millis(rand::thread_rng().gen_range(300..400)), // Adjust as needed
            my_address: my_address.clone(),
            grpc_address,
            reset_heartbeat_signal: tokio::sync::Notify::new(),
            blockchain_client: Mutex::new(None),
        };
        let consensus = Arc::new(consensus);

        Self::initialize_periodic_peer_discovery(Arc::clone(&consensus));
        Self::initialize_transaction_execution_queue(Arc::clone(&consensus));
        Self::initialize_append_entries_channel(Arc::clone(&consensus), rx_pending_txs, rx_blocks);
        Self::initialize_server(Arc::clone(&consensus));
        Self::initialize_heartbeat_timer(Arc::clone(&consensus));

        tracing::info!(my_address = %my_address, "consensus module initialized");
        consensus
    }

    fn discover_my_address(jsonrpc_port: u16, grpc_port: u16) -> PeerAddress {
        let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        socket.connect("8.8.8.8:80").ok().unwrap();
        let my_ip = socket.local_addr().ok().map(|addr| addr.ip().to_string()).unwrap();

        PeerAddress::new(format!("http://{}", my_ip), jsonrpc_port, grpc_port)
    }

    /// Initializes the heartbeat and election timers.
    /// This function periodically checks if the node should start a new election based on the election timeout.
    /// The timer is reset when an `AppendEntries` request is received, ensuring the node remains a follower if a leader is active.
    ///
    /// When there are healthy peers we need to wait for the grace period of discovery
    /// to avoid starting an election too soon (due to the leader not being discovered yet)
    fn initialize_heartbeat_timer(consensus: Arc<Consensus>) {
        spawn_named("consensus::heartbeat_timer", async move {
            discovery::discover_peers(Arc::clone(&consensus)).await;
            if consensus.peers.read().await.is_empty() {
                tracing::info!("no peers, starting hearbeat timer immediately");
                Self::start_election(Arc::clone(&consensus)).await;
            } else {
                traced_sleep(PEER_DISCOVERY_DELAY, SleepReason::Interval).await;
                tracing::info!("waiting for peer discovery grace period");
            }

            let timeout = consensus.heartbeat_timeout;
            loop {
                tokio::select! {
                    _ = traced_sleep(timeout, SleepReason::Interval) => {
                        if !consensus.is_leader() {
                            tracing::info!("starting election due to heartbeat timeout");
                            Self::start_election(Arc::clone(&consensus)).await;
                        } else {
                            let current_term = consensus.current_term.load(Ordering::SeqCst);
                            tracing::info!(current_term = current_term, "heartbeat timeout reached, but I am the leader, so we ignore the election");
                        }
                    },
                    _ = consensus.reset_heartbeat_signal.notified() => {
                        // Timer reset upon receiving AppendEntries
                        tracing::info!("resetting election timer due to AppendEntries");
                    },
                }
            }
        });
    }

    /// Starts the election process for the consensus module.
    ///
    /// This method is called when a node suspects that there is no active leader in the cluster.
    /// The node increments its term and votes for itself, then sends RequestVote RPCs to all other nodes in the cluster.
    /// If the node receives a majority of votes, it becomes the leader. Otherwise, it remains a follower and waits for the next election timeout.
    ///
    /// # Details
    ///
    /// - The method first increments the current term and votes for itself.
    /// - It then sends out `RequestVote` RPCs to all known peers.
    /// - If a majority of the peers grant their votes, the node transitions to the leader role.
    /// - If not, it remains a follower and waits for the next election cycle.
    async fn start_election(consensus: Arc<Consensus>) {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        discovery::discover_peers(Arc::clone(&consensus)).await;

        let term = consensus.current_term.fetch_add(1, Ordering::SeqCst) + 1;
        consensus.current_term.store(term, Ordering::SeqCst);

        *consensus.voted_for.lock().await = Some(consensus.my_address.clone());

        let mut votes = 1; // Vote for self

        tracing::info!(
            requested_term = term,
            candidate_id = %consensus.my_address,
            "requesting vote on election for {} peers",
            consensus.peers.read().await.len()
        );
        let peers = consensus.peers.read().await;
        for (peer_address, (peer, _)) in peers.iter() {
            let mut peer_clone = peer.clone();

            let request = Request::new(RequestVoteRequest {
                term,
                candidate_id: consensus.my_address.to_string(),
                last_log_index: consensus.last_arrived_block_number.load(Ordering::SeqCst),
                last_log_term: term,
            });

            match peer_clone.client.request_vote(request).await {
                Ok(response) => {
                    let response_inner = response.into_inner();
                    if response_inner.vote_granted {
                        let current_term = consensus.current_term.load(Ordering::SeqCst);
                        if response_inner.term == current_term {
                            tracing::info!(peer_address = %peer_address, "received vote on election");
                            votes += 1;
                        } else {
                            // this usually happens when we have either a split brain or a network issue, maybe both
                            tracing::error!(peer_address = %peer_address, expected_term = response_inner.term, "received vote on election with different term");
                        }
                    } else {
                        tracing::info!(peer_address = %peer_address, "did not receive vote on election");
                    }
                }
                Err(_) => {
                    tracing::warn!("failed to request vote on election from {:?}", peer_address);
                }
            }
        }

        let total_nodes = peers.len() + 1; // Including self
        let majority = total_nodes / 2 + 1;

        if votes >= majority {
            tracing::info!(votes = votes, peers = peers.len(), term = term, "became the leader on election");
            consensus.become_leader().await;
        } else {
            tracing::info!(votes = votes, peers = peers.len(), term = term, "failed to become the leader on election");
            consensus.set_role(Role::Follower);
        }

        #[cfg(feature = "metrics")]
        metrics::inc_consensus_start_election(start.elapsed());
    }

    /// When importer config is set, it will refresh the blockchain client (substrate or any other blockchain client)
    /// however, if stratus is on miner mode, it will clear the blockchain client for safety reasons, so it has no chance to forward anything to a follower node
    async fn become_leader(&self) {
        // checks if it's running on importer-online mode
        if self.importer_config.is_some() {
            self.refresh_blockchain_client().await;
        } else {
            let mut blockchain_client_lock = self.blockchain_client.lock().await;
            *blockchain_client_lock = None; // clear the blockchain client for safety reasons when not running on importer-online mode
        }

        self.set_role(Role::Leader);
    }

    async fn refresh_blockchain_client(&self) {
        let (http_url, _) = self.get_chain_url().await.expect("failed to get chain url");
        let mut blockchain_client_lock = self.blockchain_client.lock().await;
        *blockchain_client_lock = Some(
            BlockchainClient::new_http(&http_url, Duration::from_secs(2))
                .await
                .expect("failed to create blockchain client")
                .into(),
        );
    }

    fn initialize_periodic_peer_discovery(consensus: Arc<Consensus>) {
        spawn_named("consensus::peer_discovery", async move {
            let mut interval = tokio::time::interval(PEER_DISCOVERY_DELAY);
            loop {
                tracing::info!("starting periodic peer discovery");
                discovery::discover_peers(Arc::clone(&consensus)).await;
                interval.tick().await;
            }
        });
    }

    fn initialize_transaction_execution_queue(consensus: Arc<Consensus>) {
        //TODO add data to consensus-log-transactions
        //TODO rediscover followers on comunication error
        //XXX FIXME deal with the scenario where a transactionHash arrives after the block, in this case before saving the block LogEntry, it should ALWAYS wait for all transaction hashes
        //TODO maybe check if I'm currently the leader?
        spawn_named("consensus::transaction_execution_queue", async move {
            let interval = Duration::from_millis(40);
            loop {
                tokio::time::sleep(interval).await;
                if consensus.is_leader() {
                    let mut queue = consensus.transaction_execution_queue.lock().await;
                    let executions = queue.drain(..).collect::<Vec<_>>();
                    drop(queue);

                    let peers = consensus.peers.read().await;
                    for (_, (peer, _)) in peers.iter() {
                        let mut peer_clone = peer.clone();
                        consensus.append_transaction_executions_to_peer(&mut peer_clone, executions.clone()).await;
                    }
                }
            }
        });
    }

    /// This channel broadcasts blocks and transactons executions to followers.
    /// Each follower has a queue of blocks and transactions to be sent at handle_peer_propagation.
    //TODO this broadcast needs to wait for majority of followers to confirm the log before sending the next one
    fn initialize_append_entries_channel(
        consensus: Arc<Consensus>,
        mut rx_pending_txs: broadcast::Receiver<TransactionExecution>,
        mut rx_blocks: broadcast::Receiver<Block>,
    ) {
        spawn_named("consensus::block_and_executions_sender", async move {
            loop {
                tokio::select! {
                    Ok(tx) = rx_pending_txs.recv() => {
                        if consensus.is_leader() {
                            tracing::info!(hash = %tx.hash(), "received transaction execution to send to followers");
                            if tx.is_local() {
                                tracing::debug!(hash = %tx.hash(), "skipping local transaction because only external transactions are supported for now");
                                continue;
                            }

                            //TODO XXX save transaction to appendEntries log
                            let transaction = vec![tx.to_append_entry_transaction()];
                            let transaction_entry = LogEntryData::TransactionExecutionEntries(transaction);
                            if consensus.broadcast_sender.send(transaction_entry).is_err() {
                                tracing::error!("failed to broadcast transaction");
                            }
                        }
                    }
                    Ok(block) = rx_blocks.recv() => {
                        if consensus.is_leader() {
                            tracing::info!(number = block.header.number.as_u64(), "received block to send to followers");

                            //TODO save block to appendEntries log
                            //TODO before saving check if all transaction_hashes are already in the log
                            let block_entry = LogEntryData::BlockEntry(block.header.to_append_entry_block_header(Vec::new()));
                            if consensus.broadcast_sender.send(block_entry).is_err() {
                                tracing::error!("failed to broadcast block");
                            }
                        }
                    }
                    else => {
                        tokio::task::yield_now().await;
                    }
                }
            }
        });
    }

    fn initialize_server(consensus: Arc<Consensus>) {
        spawn_named("consensus::server", async move {
            tracing::info!("Starting append entry service at address: {}", consensus.grpc_address);
            let addr = consensus.grpc_address;

            let append_entry_service = AppendEntryServiceImpl {
                consensus: Mutex::new(consensus),
            };

            let server = Server::builder()
                .add_service(AppendEntryServiceServer::new(append_entry_service))
                .serve(addr)
                .await;

            if let Err(e) = server {
                let message = GlobalState::shutdown_from("consensus", &format!("failed to create server at {}", addr));
                tracing::error!(reason = ?e, %message);
            }
        });
    }

    fn set_role(&self, role: Role) {
        self.role.store(role as u8, Ordering::SeqCst);

        #[cfg(feature = "metrics")]
        {
            if role == Role::Leader {
                metrics::set_consensus_is_leader(1_u64);
                metrics::inc_consensus_leadership_change();
            } else {
                metrics::set_consensus_is_leader(0_u64);
            }
        }
    }

    //FIXME TODO automate the way we gather the leader, instead of using a env var
    pub fn is_leader(&self) -> bool {
        self.role.load(Ordering::SeqCst) == Role::Leader as u8
    }

    pub async fn is_follower(&self) -> bool {
        self.role.load(Ordering::SeqCst) == Role::Follower as u8
    }

    pub fn should_forward(&self) -> bool {
        let is_leader = self.is_leader();
        tracing::info!(
            is_leader = is_leader,
            sync_online_enabled = self.importer_config.is_some(),
            "handling request forward"
        );
        if is_leader && self.importer_config.is_none() {
            return false; // the leader is on miner mode and should deal with the requests
        }
        true
    }

    pub async fn forward(&self, transaction: TransactionInput) -> anyhow::Result<Hash> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let blockchain_client_lock = self.blockchain_client.lock().await;

        let Some(ref blockchain_client) = *blockchain_client_lock else {
            return Err(anyhow::anyhow!("blockchain client is not set, cannot forward transaction"));
        };

        let forward_to = forward_to::TransactionRelayer::new(Arc::clone(blockchain_client));
        let result = forward_to.forward(transaction).await?;

        #[cfg(feature = "metrics")]
        metrics::inc_consensus_forward(start.elapsed());

        Ok(result.tx_hash) //XXX HEX
    }

    //TODO for now the block number is the index, but it should be a separate index wiht the execution AND the block
    pub async fn should_serve(&self) -> bool {
        if self.is_leader() {
            return true;
        }

        let blockchain_client_lock = self.blockchain_client.lock().await;
        if blockchain_client_lock.is_none() {
            tracing::warn!("blockchain client is not set, cannot serve requests because they cant be forwarded");
            return false;
        }

        let last_arrived_block_number = self.last_arrived_block_number.load(Ordering::SeqCst);

        if last_arrived_block_number == 0 {
            tracing::warn!("no appendEntry has been received yet");
            return false;
        }

        let storage_block_number: u64 = self.storage.read_mined_block_number().unwrap_or(BlockNumber::from(0)).into();

        tracing::info!(
            "last arrived block number: {}, storage block number: {}",
            last_arrived_block_number,
            storage_block_number
        );

        if (last_arrived_block_number - 3) <= storage_block_number {
            tracing::info!("should serve request");
            true
        } else {
            let diff = (last_arrived_block_number as i128) - (storage_block_number as i128);
            tracing::warn!(diff = diff, "should not serve request");
            false
        }
    }

    #[cfg(feature = "kubernetes")]
    fn current_node() -> Option<String> {
        let pod_name = env::var("MY_POD_NAME").ok()?;
        Some(pod_name.trim().to_string())
    }

    #[cfg(feature = "kubernetes")]
    fn current_namespace() -> Option<String> {
        let namespace = env::var("NAMESPACE").ok()?;
        Some(namespace.trim().to_string())
    }

    async fn leader_address(&self) -> anyhow::Result<PeerAddress> {
        let peers = self.peers.read().await;
        for (address, (peer, _)) in peers.iter() {
            if peer.role == Role::Leader {
                return Ok(address.clone());
            }
        }
        Err(anyhow!("Leader not found"))
    }

    pub async fn get_chain_url(&self) -> anyhow::Result<(String, Option<String>)> {
        if let Some(importer_config) = self.importer_config.clone() {
            return Ok((importer_config.online.external_rpc, importer_config.online.external_rpc_ws));
        }

        if self.is_follower().await {
            if let Ok(leader_address) = self.leader_address().await {
                return Ok((leader_address.full_jsonrpc_address(), None));
            }
        }

        Err(anyhow!("tried to get chain url as a leader and while running on miner mode"))
    }

    async fn update_leader(&self, leader_address: PeerAddress) {
        if leader_address == self.leader_address().await.unwrap_or_default() {
            tracing::info!("leader is the same as before");
            return;
        }

        let mut peers = self.peers.write().await;

        for (address, (peer, _)) in peers.iter_mut() {
            if *address == leader_address {
                peer.role = Role::Leader;

                self.refresh_blockchain_client().await;
            } else {
                peer.role = Role::Follower;
            }
        }

        tracing::info!(leader = %leader_address, "updated leader information");
    }

    /// Handles the propagation of log entries to peers in the consensus network.
    async fn handle_peer_propagation(mut peer: Peer, consensus: Arc<Consensus>) {
        let mut log_entry_queue: Vec<LogEntryData> = Vec::new();
        loop {
            let mut receiver_lock = peer.receiver.lock().await;
            match receiver_lock.recv().await {
                Ok(log_entry) => {
                    log_entry_queue.push(log_entry);
                }
                Err(e) => {
                    tracing::warn!("Error receiving log entry for peer {:?}: {:?}", peer.client, e);
                }
            }
            drop(receiver_lock);

            while let Some(log_entry) = log_entry_queue.first() {
                match log_entry {
                    LogEntryData::BlockEntry(block) => {
                        tracing::info!("sending block to peer: {:?}", peer.client);
                        match consensus.append_block_to_peer(&mut peer, block).await {
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
                        let mut queue = consensus.transaction_execution_queue.lock().await;
                        queue.extend(transaction_executions.clone());
                        log_entry_queue.remove(0);
                    }
                    LogEntryData::EmptyData => {
                        tracing::warn!("empty log entry received");
                        log_entry_queue.remove(0);
                    }
                }
            }
        }
    }

    async fn append_transaction_executions_to_peer(&self, peer: &mut Peer, executions: Vec<TransactionExecutionEntry>) {
        if self.is_leader() {
            let current_term = self.current_term.load(Ordering::SeqCst);
            let request = Request::new(AppendTransactionExecutionsRequest {
                term: current_term,
                prev_log_index: self.last_arrived_block_number.load(Ordering::SeqCst), //FIXME we should gather it from the log entries
                prev_log_term: current_term,                                           //FIXME we should gather it from the log entries
                executions,
                leader_id: self.my_address.to_string(),
            });

            match peer.client.append_transaction_executions(request).await {
                Ok(response) =>
                    if response.into_inner().status == StatusCode::AppendSuccess as i32 {
                        tracing::info!("Successfully appended transaction executions to peer: {:?}", peer.client);
                    } else {
                        tracing::warn!("Failed to append transaction executions to peer: {:?}", peer.client);
                    },
                Err(e) => {
                    tracing::warn!("Error appending transaction executions to peer {:?}: {:?}", peer.client, e);
                }
            }
        } else {
            tracing::error!(
                transactions = executions.len(),
                "append_transaction_executions_to_peer called on non-leader node"
            );
        }
    }

    async fn append_block_to_peer(&self, peer: &mut Peer, block_entry: &BlockEntry) -> Result<(), anyhow::Error> {
        if self.is_leader() {
            #[cfg(feature = "metrics")]
            let start = metrics::now();

            let current_term = self.current_term.load(Ordering::SeqCst);
            let request = Request::new(AppendBlockCommitRequest {
                term: current_term,
                prev_log_index: self.last_arrived_block_number.load(Ordering::SeqCst), //FIXME we should gather it from the log entries
                prev_log_term: current_term,                                           //FIXME we should gather it from the log entries
                block_entry: Some(block_entry.clone()),
                leader_id: self.my_address.to_string(),
            });

            let response = peer.client.append_block_commit(request).await?;
            let response = response.into_inner();

            tracing::info!(match_index = peer.match_index, next_index = peer.next_index, role = ?peer.role,  "current follower state on election"); //TODO also move this to metrics

            #[cfg(feature = "metrics")]
            metrics::inc_consensus_append_block_to_peer(start.elapsed());

            match StatusCode::try_from(response.status) {
                Ok(StatusCode::AppendSuccess) => Ok(()),
                _ => Err(anyhow!("Unexpected status code: {:?}", response.status)),
            }
        } else {
            tracing::error!("append_block_to_peer called on non-leader node");
            Err(anyhow!("append_block_to_peer called on non-leader node"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub mod factories;
    mod test_simple_blocks;

    #[test]
    fn test_peer_address_from_string_valid_http() {
        let input = "http://127.0.0.1:3000;3777".to_string();
        let result = PeerAddress::from_string(input);

        assert!(result.is_ok());
        let peer_address = result.unwrap();
        assert_eq!(peer_address.address, "http://127.0.0.1");
        assert_eq!(peer_address.jsonrpc_port, 3000);
        assert_eq!(peer_address.grpc_port, 3777);
    }

    #[test]
    fn test_peer_address_from_string_valid_https() {
        let input = "https://127.0.0.1:3000;3777".to_string();
        let result = PeerAddress::from_string(input);

        assert!(result.is_ok());
        let peer_address = result.unwrap();
        assert_eq!(peer_address.address, "https://127.0.0.1");
        assert_eq!(peer_address.jsonrpc_port, 3000);
        assert_eq!(peer_address.grpc_port, 3777);
    }

    #[test]
    fn test_peer_address_from_string_invalid_format() {
        let input = "http://127.0.0.1-3000;3777".to_string();
        let result = PeerAddress::from_string(input);

        assert!(result.is_err());
        assert_eq!(result.err().unwrap().to_string(), "invalid format");
    }

    #[test]
    fn test_peer_address_from_string_missing_scheme() {
        let input = "127.0.0.1:3000;3777".to_string();
        let result = PeerAddress::from_string(input);

        assert!(result.is_err());
        assert_eq!(result.err().unwrap().to_string(), "invalid scheme");
    }

    #[test]
    fn test_peer_address_full_grpc_address() {
        let peer_address = PeerAddress::new("127.0.0.1".to_string(), 3000, 3777);
        assert_eq!(peer_address.full_grpc_address(), "127.0.0.1:3777");
    }
}
