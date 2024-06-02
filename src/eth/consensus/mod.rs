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
use tokio::sync::mpsc::Sender;
use tokio::sync::mpsc::{self};
use tokio::sync::Mutex;
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
use crate::infra::BlockchainClient;

pub mod append_entry {
    tonic::include_proto!("append_entry");
}

use append_entry::append_entry_service_client::AppendEntryServiceClient;
use append_entry::append_entry_service_server::AppendEntryService;
use append_entry::append_entry_service_server::AppendEntryServiceServer;
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

const RETRY_ATTEMPTS: u32 = 3;
const RETRY_DELAY: Duration = Duration::from_millis(10);

#[derive(Clone, Debug, PartialEq)]
enum Role {
    _Leader, //TODO implement leader election
    Follower,
    _Candidate,
}

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
struct PeerAddress(String);

impl PeerAddress {
    pub fn inner(&self) -> &str {
        &self.0
    }
}

#[derive(Clone)]
struct Peer {
    client: AppendEntryServiceClient<Channel>,
    last_heartbeat: std::time::Instant, //TODO implement metrics for this
    match_index: u64,
    next_index: u64,
    role: Role,
    term: u64,
}

pub struct Consensus {
    pub sender: Sender<Block>,
    importer_config: Option<RunWithImporterConfig>, //HACK this is used with sync online only
    storage: Arc<StratusStorage>,
    peers: Arc<Mutex<HashMap<PeerAddress, Peer>>>,
    leader_name: String,                  //XXX check the peers instead of using it
    last_arrived_block_number: AtomicU64, //TODO use a true index for both executions and blocks, currently we use something like Bully algorithm so block number is fine
}

impl Consensus {
    //XXX for now we pick the leader name from the environment
    // the correct is to have a leader election algorithm
    pub async fn new(storage: Arc<StratusStorage>, importer_config: Option<RunWithImporterConfig>) -> Arc<Self> {
        if Self::is_stand_alone() {
            tracing::info!("No consensus module available, running in standalone mode");
            return Self::new_stand_alone(storage, importer_config);
        };

        let leader_name = match importer_config.clone() {
            Some(config) => config.leader_node.unwrap_or_default(),
            None => "unknown".to_string(),
        };

        tracing::info!(leader_name = leader_name, "Starting consensus module with leader");

        let (sender, receiver) = mpsc::channel::<Block>(32);
        let receiver = Arc::new(Mutex::new(receiver));

        let last_arrived_block_number = AtomicU64::new(storage.read_mined_block_number().await.unwrap_or(BlockNumber::from(0)).into());
        let peers = Arc::new(Mutex::new(HashMap::new()));

        let consensus = Self {
            sender,
            storage,
            peers,
            leader_name,
            importer_config,
            last_arrived_block_number,
        };
        let consensus = Arc::new(consensus);

        let _ = Self::discover_peers(Arc::clone(&consensus)).await; //TODO refactor this method to also receive addresses from the environment
        Self::initialize_append_entries_channel(Arc::clone(&consensus), Arc::clone(&receiver));
        Self::initialize_server(Arc::clone(&consensus));

        consensus
    }

    fn new_stand_alone(storage: Arc<StratusStorage>, importer_config: Option<RunWithImporterConfig>) -> Arc<Self> {
        let (sender, mut receiver) = mpsc::channel::<Block>(32);

        tokio::spawn(async move {
            while let Some(data) = channel_read!(receiver) {
                tracing::info!(number = data.header.number.as_u64(), "Received block");
            }
        });

        let last_arrived_block_number = AtomicU64::new(0);
        let peers = Arc::new(Mutex::new(HashMap::new()));

        Arc::new(Self {
            leader_name: "standalone".to_string(),
            storage,
            sender,
            peers,
            last_arrived_block_number,
            importer_config,
        })
    }

    fn initialize_append_entries_channel(consensus: Arc<Consensus>, receiver: Arc<Mutex<mpsc::Receiver<Block>>>) {
        tokio::spawn(async move {
            let peers = consensus.peers.lock().await;

            loop {
                let mut receiver_lock = receiver.lock().await;
                if let Some(data) = receiver_lock.recv().await {
                    if consensus.is_leader() {
                        //TODO add data to consensus-log-transactions
                        //TODO at the begining of temp-storage, load the consensus-log-transactions so the index becomes clear
                        tracing::info!(number = data.header.number.as_u64(), "received block to send to followers");

                        //TODO use gRPC instead of jsonrpc
                        //FIXME for now, this has no colateral efects, but it will have in the future
                        match Self::append_block_commit_to_followers(data.clone(), peers.clone()).await {
                            Ok(_) => {
                                tracing::info!(number = data.header.number.as_u64(), "Data sent to followers");
                            }
                            Err(e) => {
                                //TODO rediscover followers on comunication error
                                tracing::error!("Failed to send data to followers: {}", e);
                            }
                        }
                    }
                }
            }
        });
    }

    fn initialize_server(consensus: Arc<Consensus>) {
        tokio::spawn(async move {
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
    pub fn is_leader(&self) -> bool {
        Self::current_node().unwrap_or("standalone".to_string()) == self.leader_name
    }

    pub fn is_follower(&self) -> bool {
        !self.is_leader()
    }

    pub fn is_stand_alone() -> bool {
        Self::current_node().is_none()
    }

    pub fn should_forward(&self) -> bool {
        tracing::info!(
            is_leader = self.is_leader(),
            sync_online_enabled = self.importer_config.is_some(),
            "handling request forward"
        );
        if self.is_leader() && self.importer_config.is_none() {
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
        if Self::is_stand_alone() {
            return true;
        }
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
        match self.importer_config.clone() {
            Some(importer_config) => Some((importer_config.online.external_rpc, importer_config.online.external_rpc_ws)),
            None => None,
        }
    }

    #[tracing::instrument(skip_all)]
    pub async fn discover_peers(consensus: Arc<Consensus>) -> Result<(), anyhow::Error> {
        let mut peers: Vec<(String, Peer)> = Vec::new();

        #[cfg(feature = "kubernetes")]
        {
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
                                role: Role::Follower,
                                term: 0, // Replace with actual term
                            };
                            peers.push((address, peer));
                        }
                    }
                }
            }
        }

        let mut peers_lock = consensus.peers.lock().await;
        for peer in &peers {
            peers_lock.insert(PeerAddress(peer.0.clone()), peer.1.clone());
        }
        tracing::info!(peers = peers.iter().map(|p| p.0.clone()).collect::<Vec<String>>().join(","), "Discovered peers",);

        Ok(())
    }

    async fn append_block_commit(
        mut follower: Peer,
        header: BlockHeader,
        transaction_hashes: Vec<String>,
        term: u64,
        prev_log_index: u64,
        prev_log_term: u64,
        peer_id: String,
    ) -> Result<(), anyhow::Error> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        for attempt in 1..=RETRY_ATTEMPTS {
            let request = Request::new(AppendBlockCommitRequest {
                term,
                prev_log_index,
                prev_log_term,
                header: Some(header.clone()),
                transaction_hashes: transaction_hashes.clone(),
            });

            let response = follower.client.append_block_commit(request).await;

            match response {
                Ok(resp) => {
                    let resp = resp.into_inner();

                    tracing::debug!(follower = peer_id, last_heartbeat = ?follower.last_heartbeat, match_index = follower.match_index, next_index = follower.next_index, role = ?follower.role, term = follower.term,  "current follower state"); //TODO also move this to metrics

                    match StatusCode::try_from(resp.status) {
                        Ok(StatusCode::AppendSuccess) => {
                            #[cfg(not(feature = "metrics"))]
                            tracing::debug!(follower = peer_id, "Block commit appended to follower: attempt {}: success", attempt);
                            #[cfg(feature = "metrics")]
                            tracing::info!(
                                follower = peer_id,
                                "Block commit appended to follower: attempt {}: success time_elapsed: {:?}",
                                attempt,
                                start.elapsed()
                            );
                            return Ok(());
                        }
                        _ => {
                            tracing::error!(follower = peer_id, "Unexpected status from follower: {:?}", resp.status);
                        }
                    }
                }
                Err(e) => tracing::error!(follower = peer_id, "Error appending block commit to follower, attempt{}: {:?}", attempt, e),
            }
            sleep(RETRY_DELAY).await;
        }

        #[cfg(feature = "metrics")]
        metrics::inc_append_entries(start.elapsed());

        Err(anyhow!("Failed to append block commit to {} after {} attempts", peer_id, RETRY_ATTEMPTS))
    }

    #[tracing::instrument(skip_all)]
    pub async fn append_block_commit_to_followers(block: Block, followers: HashMap<PeerAddress, Peer>) -> Result<(), anyhow::Error> {
        let header: BlockHeader = (&block.header).into();
        let transaction_hashes = vec![]; // Replace with actual transaction hashes

        let term = 0; // Populate with actual term
        let prev_log_index = 0; // Populate with actual previous log index
        let prev_log_term = 0; // Populate with actual previous log term

        #[cfg(feature = "metrics")]
        let start = metrics::now();
        for follower in &followers {
            if let Err(e) = Self::append_block_commit(
                follower.1.clone(),
                header.clone(),
                transaction_hashes.clone(),
                term,
                prev_log_index,
                prev_log_term,
                follower.0.inner().to_string(),
            )
            .await
            {
                tracing::warn!(follower = follower.0.inner().to_string(), "Error appending block commit to follower: {:?}", e);
            }
        }

        #[cfg(feature = "metrics")]
        metrics::inc_append_entries(start.elapsed());

        Ok(())
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
}
