#[cfg(feature = "kubernetes")]
pub mod consensus_kube {
    use std::env;
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use std::time::Duration;

    use anyhow::anyhow;
    use k8s_openapi::api::core::v1::Pod;
    use kube::api::Api;
    use kube::api::ListParams;
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

    use crate::config::RunWithImporterConfig;
    use crate::eth::primitives::Block;
    #[cfg(feature = "metrics")]
    use crate::infra::metrics;

    const RETRY_ATTEMPTS: u32 = 3;
    const RETRY_DELAY: Duration = Duration::from_millis(10);

    #[derive(Clone)]
    struct Peer {
        address: String,
        client: AppendEntryServiceClient<Channel>,
    }

    pub struct Consensus {
        pub sender: Sender<Block>,
        leader_name: String,                  //XXX check the peers instead of using it
        last_arrived_block_number: AtomicU64, //TODO use a true index for both executions and blocks, currently we use something like Bully algorithm so block number is fine
    }

    impl Consensus {
        //XXX for now we pick the leader name from the environment
        // the correct is to have a leader election algorithm
        pub fn new(leader_name: Option<String>) -> Arc<Self> {
            let Some(_node_name) = Self::current_node() else {
                tracing::info!("No consensus module available, running in standalone mode");
                return Self::new_stand_alone();
            };

            let Some(leader_name) = leader_name else {
                tracing::info!("No leader name provided, running in standalone mode");
                return Self::new_stand_alone();
            };

            tracing::info!("Starting consensus module with leader: {}", leader_name);

            let (sender, receiver) = mpsc::channel::<Block>(32);
            let receiver = Arc::new(Mutex::new(receiver));

            let last_arrived_block_number = AtomicU64::new(0); //TODO load from consensus storage

            let consensus = Self {
                leader_name,
                sender,
                last_arrived_block_number,
            };
            let consensus = Arc::new(consensus);

            Self::initialize_append_entries_channel(Arc::clone(&consensus), Arc::clone(&receiver));
            Self::initialize_server(Arc::clone(&consensus));

            consensus
        }

        fn new_stand_alone() -> Arc<Self> {
            let (sender, mut receiver) = mpsc::channel::<Block>(32);

            tokio::spawn(async move {
                while let Some(data) = channel_read!(receiver) {
                    tracing::info!(number = data.header.number.as_u64(), "Received block");
                }
            });

            let last_arrived_block_number = AtomicU64::new(0);

            Arc::new(Self {
                leader_name: "standalone".to_string(),
                sender,
                last_arrived_block_number,
            })
        }

        fn initialize_append_entries_channel(consensus_channel: Arc<Consensus>, receiver: Arc<Mutex<mpsc::Receiver<Block>>>) {
            tokio::spawn(async move {
                let followers = Self::discover_followers().await.unwrap_or_default();

                tracing::info!(
                    "Discovered followers: {}",
                    followers.iter().map(|f| f.address.to_string()).collect::<Vec<String>>().join(", ")
                );

                loop {
                    let mut receiver_lock = receiver.lock().await;
                    if let Some(data) = receiver_lock.recv().await {
                        if consensus_channel.is_leader() {
                            //TODO add data to consensus-log-transactions
                            //TODO at the begining of temp-storage, load the consensus-log-transactions so the index becomes clear
                            tracing::info!(number = data.header.number.as_u64(), "received block to send to followers");

                            //TODO use gRPC instead of jsonrpc
                            //FIXME for now, this has no colateral efects, but it will have in the future
                            match Self::append_block_commit_to_followers(data.clone(), followers.clone()).await {
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
            Self::current_node().unwrap_or("".to_string()) == self.leader_name
        }

        pub fn is_follower(&self) -> bool {
            !self.is_leader()
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
        pub fn get_chain_url(&self, config: RunWithImporterConfig) -> (String, Option<String>) {
            if self.is_follower() {
                if let Some(namespace) = Self::current_namespace() {
                    return (format!("http://{}.stratus-api.{}.svc.cluster.local:3000", self.leader_name, namespace), None);
                }
            }
            (config.online.external_rpc, config.online.external_rpc_ws)
        }

        #[tracing::instrument(skip_all)]
        pub async fn discover_followers() -> Result<Vec<Peer>, anyhow::Error> {
            let client = Client::try_default().await?;
            let pods: Api<Pod> = Api::namespaced(client, &Self::current_namespace().unwrap_or("default".to_string()));

            let lp = ListParams::default().labels("app=stratus-api");
            let pod_list = pods.list(&lp).await?;

            let mut followers = Vec::new();
            for p in pod_list.items {
                if let Some(pod_name) = p.metadata.name {
                    if pod_name != Self::current_node().unwrap() {
                        if let Some(namespace) = Self::current_namespace() {
                            let address = format!("http://{}.stratus-api.{}.svc.cluster.local:3777", pod_name, namespace);
                            let client = AppendEntryServiceClient::connect(address.clone()).await?;

                            let peer = Peer { address, client };
                            followers.push(peer);
                        }
                    }
                }
            }

            Ok(followers)
        }

        async fn append_block_commit(
            mut follower: Peer,
            header: BlockHeader,
            transaction_hashes: Vec<String>,
            term: u64,
            prev_log_index: u64,
            prev_log_term: u64,
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
                        match StatusCode::try_from(resp.status) {
                            Ok(StatusCode::AppendSuccess) => {
                                #[cfg(not(feature = "metrics"))]
                                tracing::debug!("Block commit appended to follower {}: attempt {}: success", follower.address, attempt);
                                #[cfg(feature = "metrics")]
                                tracing::debug!(
                                    "Block commit appended to follower {}: attempt {}: success time_elapsed: {:?}",
                                    follower.address,
                                    attempt,
                                    start.elapsed()
                                );
                                return Ok(());
                            }
                            _ => {
                                tracing::error!("Unexpected status from follower {}: {:?}", follower.address, resp.status);
                            }
                        }
                    }
                    Err(e) => tracing::error!("Error appending block commit to follower {}: attempt {}: {:?}", follower.address, attempt, e),
                }
                sleep(RETRY_DELAY).await;
            }

            #[cfg(feature = "metrics")]
            metrics::inc_append_entries(start.elapsed());

            Err(anyhow!(
                "Failed to append block commit to {} after {} attempts",
                follower.address,
                RETRY_ATTEMPTS
            ))
        }

        #[tracing::instrument(skip_all)]
        pub async fn append_block_commit_to_followers(block: Block, followers: Vec<Peer>) -> Result<(), anyhow::Error> {
            let header: BlockHeader = (&block.header).into();
            let transaction_hashes = vec!["hash1".to_string(), "hash2".to_string()]; // Replace with actual transaction hashes

            let term = 0; // Populate with actual term
            let prev_log_index = 0; // Populate with actual previous log index
            let prev_log_term = 0; // Populate with actual previous log term

            #[cfg(feature = "metrics")]
            let start = metrics::now();
            for follower in &followers {
                if let Err(e) = Self::append_block_commit(
                    follower.clone(),
                    header.clone(),
                    transaction_hashes.clone(),
                    term,
                    prev_log_index,
                    prev_log_term,
                )
                .await
                {
                    tracing::debug!("Error appending block commit to follower {}: {:?}", follower.address, e);
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

            Ok(Response::new(AppendBlockCommitResponse {
                status: StatusCode::AppendSuccess as i32,
                message: "Block Commit appended successfully".into(),
                last_committed_block_number: consensus.last_arrived_block_number.load(Ordering::SeqCst),
            }))
        }
    }
}

#[cfg(not(feature = "kubernetes"))]
pub mod consensus_mock {
    use std::sync::Arc;

    use tokio::sync::mpsc::Sender;

    use crate::config::RunWithImporterConfig;
    use crate::eth::primitives::Block;

    pub struct Consensus {
        pub sender: Sender<Block>,
    }

    impl Consensus {
        pub fn new(_leader_name: Option<String>) -> Arc<Self> {
            todo!()
        }

        pub fn get_chain_url(&self, _config: RunWithImporterConfig) -> (String, Option<String>) {
            todo!()
        }
    }
}
