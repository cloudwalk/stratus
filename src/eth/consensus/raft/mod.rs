//FIXME temporarily there are a lot of allow dead_code and allow unused_imports, we need to deal with them properly later
#[allow(dead_code)] //TODO remove this
mod append_log_entries_storage;
mod discovery;
mod log_entry;
mod propagation;
pub mod utils;

mod server;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::net::UdpSocket;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use async_trait::async_trait;
use rand::Rng;
use tokio::sync::broadcast;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tonic::Request;

use crate::eth::primitives::Hash;
#[allow(unused_imports)]
use crate::eth::primitives::TransactionExecution;
use crate::eth::storage::StratusStorage;
use crate::ext::spawn_named;
use crate::ext::traced_sleep;
use crate::ext::SleepReason;
use crate::infra::BlockchainClient;
use crate::GlobalState;

pub mod append_entry {
    #![allow(clippy::default_trait_access, clippy::wildcard_imports)]
    tonic::include_proto!("append_entry");
}
#[allow(unused_imports)]
use append_entry::append_entry_service_client::AppendEntryServiceClient;
use append_entry::append_entry_service_server::AppendEntryService;
use append_entry::RequestVoteRequest;
use append_entry::TransactionExecutionEntry;

use self::append_log_entries_storage::AppendLogEntriesStorage;
use self::log_entry::LogEntryData;
use super::Consensus;
use crate::config::RunWithImporterConfig;
use crate::eth::miner::Miner;
use crate::eth::primitives::Block;
use crate::eth::primitives::Bytes;
#[cfg(feature = "metrics")]
use crate::infra::metrics;

const PEER_DISCOVERY_DELAY: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Role {
    Leader = 1,
    Follower = 2,
    _Candidate = 3,
}

static ROLE: AtomicU8 = AtomicU8::new(Role::Follower as u8);

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
use crate::eth::consensus::raft::tests::factories::MockAppendEntryServiceClient;

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

pub struct Raft {
    broadcast_sender: broadcast::Sender<LogEntryData>, //propagates the blocks
    importer_config: Option<RunWithImporterConfig>,    //HACK this is used with sync online only
    storage: Arc<StratusStorage>,
    miner: Arc<Miner>,
    log_entries_storage: Arc<AppendLogEntriesStorage>,
    peers: Arc<RwLock<HashMap<PeerAddress, PeerTuple>>>,
    #[allow(dead_code)]
    direct_peers: Vec<String>,
    voted_for: Mutex<Option<PeerAddress>>, //essential to ensure that a server only votes once per term
    current_term: AtomicU64,
    last_arrived_block_number: AtomicU64, // kept for should_serve method check
    prev_log_index: AtomicU64,
    transaction_execution_queue: Arc<Mutex<Vec<TransactionExecutionEntry>>>,
    heartbeat_timeout: Duration,
    my_address: PeerAddress,
    #[allow(dead_code)]
    grpc_address: SocketAddr,
    reset_heartbeat_signal: tokio::sync::Notify,
    blockchain_client: Mutex<Option<Arc<BlockchainClient>>>,
}

impl Raft {
    #[allow(clippy::too_many_arguments)] //TODO: refactor into consensus config
    pub fn new(
        storage: Arc<StratusStorage>,
        miner: Arc<Miner>,
        log_storage_path: Option<String>,
        direct_peers: Vec<String>,
        importer_config: Option<RunWithImporterConfig>,
        jsonrpc_address: SocketAddr,
        grpc_address: SocketAddr,
    ) -> Arc<Self> {
        let (broadcast_sender, _) = broadcast::channel(32); //TODO rename to internal_peer_broadcast_sender
        let last_arrived_block_number = AtomicU64::new(0);
        let peers = Arc::new(RwLock::new(HashMap::new()));
        let my_address = Self::discover_my_address(jsonrpc_address.port(), grpc_address.port());

        let log_entries_storage: Arc<AppendLogEntriesStorage> = Arc::new(AppendLogEntriesStorage::new(log_storage_path).unwrap());
        let current_term: u64 = log_entries_storage.get_last_term().unwrap_or(1);
        let prev_log_index: u64 = log_entries_storage.get_last_index().unwrap_or(0);

        let consensus = Self {
            broadcast_sender,
            miner: Arc::clone(&miner),
            storage,
            log_entries_storage,
            peers,
            direct_peers,
            current_term: AtomicU64::new(current_term),
            voted_for: Mutex::new(None),
            prev_log_index: AtomicU64::new(prev_log_index),
            last_arrived_block_number,
            transaction_execution_queue: Arc::new(Mutex::new(Vec::new())),
            importer_config,
            heartbeat_timeout: Duration::from_millis(rand::thread_rng().gen_range(300..400)), // Adjust as needed
            my_address: my_address.clone(),
            grpc_address,
            reset_heartbeat_signal: tokio::sync::Notify::new(),
            blockchain_client: Mutex::new(None),
        };
        let consensus = Arc::new(consensus);

        Self::initialize_periodic_peer_discovery(Arc::clone(&consensus));
        #[cfg(feature = "raft")]
        {
            //TODO replace this for a synchronous call
            let rx_pending_txs: broadcast::Receiver<TransactionExecution> = miner.notifier_pending_txs.subscribe();
            let rx_blocks: broadcast::Receiver<Block> = miner.notifier_blocks.subscribe();
            propagation::initialize_transaction_execution_queue(Arc::clone(&consensus));
            propagation::initialize_append_entries_channel(Arc::clone(&consensus), rx_pending_txs, rx_blocks);
            server::initialize_server(Arc::clone(&consensus));
        }
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
    fn initialize_heartbeat_timer(consensus: Arc<Raft>) {
        const TASK_NAME: &str = "consensus::heartbeat_timer";
        spawn_named(TASK_NAME, async move {
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
                    _ = GlobalState::wait_shutdown_warn(TASK_NAME) => {
                        return;
                    },
                    _ = traced_sleep(timeout, SleepReason::Interval) => {
                        if !Self::is_leader() {
                            tracing::info!("starting election due to heartbeat timeout");
                            Self::start_election(Arc::clone(&consensus)).await;
                        } else {
                            let current_term = consensus.current_term.load(Ordering::SeqCst);
                            tracing::info!(current_term = current_term, "heartbeat timeout reached, but I am the leader, so we ignore the election");
                        }
                    },
                    _ = consensus.reset_heartbeat_signal.notified() => {
                        // Timer reset upon receiving AppendEntries
                        match consensus.leader_address().await {
                            Ok(leader_address) => tracing::info!(leader_address = %leader_address, "resetting election timer due to AppendEntries"),
                            Err(e) => tracing::warn!(error = %e, "resetting election timer due to AppendEntries, but leader not found"), // this should not happen, but if it does it's because the leader changed in the middle of an append entry
                        }
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
    async fn start_election(consensus: Arc<Raft>) {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        discovery::discover_peers(Arc::clone(&consensus)).await;

        let term = consensus.current_term.fetch_add(1, Ordering::SeqCst) + 1;
        consensus.current_term.store(term, Ordering::SeqCst);

        *consensus.voted_for.lock().await = Some(consensus.my_address.clone());

        let mut votes = 1; // Vote for self

        let peer_addresses = {
            let peers = consensus.peers.read().await;
            peers.keys().cloned().collect::<Vec<_>>()
        };

        tracing::info!(
            requested_term = term,
            candidate_id = %consensus.my_address,
            "requesting vote on election for {} peers",
            peer_addresses.len()
        );

        for peer_address in peer_addresses {
            let peer_clone = {
                let peers = consensus.peers.read().await;
                peers.get(&peer_address).map(|(p, _)| p.clone())
            };

            if let Some(mut peer) = peer_clone {
                let request = Request::new(RequestVoteRequest {
                    term,
                    candidate_id: consensus.my_address.to_string(),
                    last_log_index: consensus.prev_log_index.load(Ordering::SeqCst),
                    last_log_term: term,
                });

                match peer.client.request_vote(request).await {
                    Ok(response) => {
                        let response_inner = response.into_inner();
                        if response_inner.vote_granted {
                            let current_term = consensus.current_term.load(Ordering::SeqCst);
                            if response_inner.term == current_term {
                                tracing::info!(peer_address = %peer_address, "received vote on election");
                                votes += 1;
                            } else {
                                // this usually happens when we have either a split brain or a network issue, maybe both
                                tracing::error!(
                                    peer_address = %peer_address,
                                    expected_term = response_inner.term,
                                    "received vote on election with different term"
                                );
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
        }

        let total_nodes = {
            let peers = consensus.peers.read().await;
            peers.len() + 1 // Including self
        };
        let majority = total_nodes / 2 + 1;

        if votes >= majority {
            tracing::info!(votes = votes, peers = total_nodes - 1, term = term, "became the leader on election");
            consensus.become_leader().await;
        } else {
            tracing::info!(votes = votes, peers = total_nodes - 1, term = term, "failed to become the leader on election");
            Self::set_role(Role::Follower);
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

        let last_index: u64 = self.log_entries_storage.get_last_index().unwrap_or(0);

        let next_index = last_index + 1;
        // When a node becomes a leader, it should reset the match_index for all peers.
        // Also, the next_index should be set to the last index + 1.
        {
            let mut peers = self.peers.write().await;
            for (peer, _) in peers.values_mut() {
                peer.match_index = 0;
                peer.next_index = next_index;
            }
        }

        Self::set_role(Role::Leader);
    }

    async fn refresh_blockchain_client(&self) {
        let (http_url, _) = self.get_chain_url().await.expect("failed to get chain url");
        let mut blockchain_client_lock = self.blockchain_client.lock().await;

        tracing::info!(http_url = http_url, "changing blockchain client");

        *blockchain_client_lock = Some(
            BlockchainClient::new_http(&http_url, Duration::from_secs(2))
                .await
                .expect("failed to create blockchain client")
                .into(),
        );
    }

    fn initialize_periodic_peer_discovery(consensus: Arc<Raft>) {
        const TASK_NAME: &str = "consensus::peer_discovery";
        spawn_named(TASK_NAME, async move {
            let mut interval = tokio::time::interval(PEER_DISCOVERY_DELAY);

            let periodic_discover = || async move {
                loop {
                    discovery::discover_peers(Arc::clone(&consensus)).await;
                    interval.tick().await;
                }
            };

            tokio::select! {
                _ = GlobalState::wait_shutdown_warn(TASK_NAME) => {},
                _ = periodic_discover() => {
                    unreachable!("this infinite future doesn't end");
                },
            };
        });
    }

    fn set_role(role: Role) {
        if ROLE.load(Ordering::SeqCst) == role as u8 {
            tracing::info!(role = ?role, "role remains the same");
            return;
        }

        tracing::info!(role = ?role, "setting role");
        ROLE.store(role as u8, Ordering::SeqCst);

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
    pub fn is_leader() -> bool {
        ROLE.load(Ordering::SeqCst) == Role::Leader as u8
    }

    pub fn is_follower() -> bool {
        ROLE.load(Ordering::SeqCst) == Role::Follower as u8
    }

    pub fn current_term(&self) -> u64 {
        self.current_term.load(Ordering::SeqCst)
    }

    pub fn last_index(&self) -> u64 {
        self.prev_log_index.load(Ordering::SeqCst)
    }

    pub fn should_forward(&self) -> bool {
        let is_leader = Self::is_leader();
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

    pub async fn forward(&self, transaction: Bytes) -> anyhow::Result<(Hash, String)> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let blockchain_client_lock = self.blockchain_client.lock().await;

        let Some(ref blockchain_client) = *blockchain_client_lock else {
            return Err(anyhow::anyhow!("blockchain client is not set, cannot forward transaction"));
        };

        let result = blockchain_client.send_raw_transaction(transaction.into()).await?;

        #[cfg(feature = "metrics")]
        metrics::inc_consensus_forward(start.elapsed());

        Ok((result.tx_hash, blockchain_client.http_url.clone())) //XXX HEX
    }

    pub async fn should_serve(&self) -> bool {
        if self.importer_config.is_some() {
            //gather the latest block number, check how far behind it is from current storage block
            //if its greater than 3 blocks of distance, it should not be served
            let blockchain_client_lock = self.blockchain_client.lock().await;

            let Some(ref blockchain_client) = *blockchain_client_lock else {
                tracing::error!("blockchain client is not set at importer, cannot serve requests because they cant be forwarded");
                return false;
            };

            let Ok(validator_block_number) = blockchain_client.fetch_block_number().await else {
                tracing::error!("unable to fetch latest block number");
                return false;
            };

            let Ok(current_block_number) = self.storage.read_mined_block_number() else {
                tracing::error!("unable to fetch current block number");
                return false;
            };

            return (validator_block_number.as_u64() - 3) <= current_block_number.as_u64();
        }

        // consensus
        if Self::is_leader() {
            return true;
        }

        if self.blockchain_client.lock().await.is_none() {
            tracing::warn!("blockchain client is not set, cannot serve requests because they cant be forwarded");
            return false;
        }

        let last_arrived_block_number = self.last_arrived_block_number.load(Ordering::SeqCst);

        if last_arrived_block_number == 0 {
            tracing::warn!("no appendEntry has been received yet");
            false
        } else {
            {
                let storage_block_number: u64 = self.storage.read_mined_block_number().unwrap_or_default().into();

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
        }
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

        if Self::is_follower() {
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
}

#[async_trait]
impl Consensus for Raft {
    async fn should_serve(&self) -> bool {
        self.should_serve().await
    }

    fn should_forward(&self) -> bool {
        self.should_forward()
    }

    async fn forward(&self, transaction: Bytes) -> anyhow::Result<Hash> {
        let (tx_hash, url) = self.forward(transaction).await?;
        tracing::info!(%tx_hash, %url, "forwarded eth_sendRawTransaction to leader");
        Ok(tx_hash)
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
