use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use codec::Decode;
use codec::Encode;
use futures::future::FutureExt;
use sc_client_api::backend;
use sc_client_api::BlockBackend;
use sc_client_api::BlockchainEvents;
use sc_client_api::ChildInfo;
use sc_client_api::CompactProof;
use sc_client_api::HeaderBackend;
use sc_client_api::KeyValueStates;
use sc_client_api::KeyValueStorageLevel;
use sc_client_api::ProofProvider;
use sc_client_api::StorageProof;
use sc_consensus::BlockCheckParams;
use sc_consensus::BlockImport;
use sc_consensus::BlockImportParams;
use sc_consensus::ImportResult;
use sc_consensus::Verifier;
use sc_network::config::EmptyTransactionPool;
use sc_network::config::MultiaddrWithPeerId;
use sc_network::config::NetworkConfiguration;
use sc_network::config::NonReservedPeerMode;
use sc_network::config::ProtocolId;
use sc_network::config::Role;
use sc_network::config::SyncMode;
use sc_network::config::TransportConfig;
use sc_network::Multiaddr;
use sc_network::PeerId;
use sp_blockchain::BlockStatus;
use sp_blockchain::Error as BlockchainError;
use sp_blockchain::Info;
use sp_consensus::error::Error as ConsensusError;
use sp_core::RuntimeDebug;
use sp_runtime::generic::BlockId;
use sp_runtime::generic::SignedBlock;
use sp_runtime::traits::BlakeTwo256;
use sp_runtime::traits::Extrinsic as ExtrinsicT;
use sp_runtime::traits::Header as HeaderT;
use sp_runtime::traits::NumberFor;
use sp_runtime::traits::Verify;
use tracing::info;

#[derive(Clone, PartialEq, Eq, Encode, Decode, RuntimeDebug, parity_util_mem::MallocSizeOf)]
pub enum Extrinsic {
    IncludeData(Vec<u8>),
    StorageChange(Vec<u8>, Option<Vec<u8>>),
}

impl serde::Serialize for Extrinsic {
    fn serialize<S>(&self, seq: S) -> Result<S::Ok, S::Error>
    where
        S: ::serde::Serializer,
    {
        self.using_encoded(|bytes| seq.serialize_bytes(bytes))
    }
}

impl ExtrinsicT for Extrinsic {
    type Call = Extrinsic;
    type SignaturePayload = ();

    fn is_signed(&self) -> Option<bool> {
        if let Extrinsic::IncludeData(_) = *self {
            Some(false)
        } else {
            Some(true)
        }
    }

    fn new(call: Self::Call, _signature_payload: Option<Self::SignaturePayload>) -> Option<Self> {
        Some(call)
    }
}
pub type BlockNumber = u64;
pub type Header = sp_runtime::generic::Header<BlockNumber, BlakeTwo256>;
pub type Block = sp_runtime::generic::Block<Header, Extrinsic>;

pub async fn serve_p2p() -> anyhow::Result<()> {
    tracing::info!("connecting to peers");

    let network_config = get_network_config().await?;
    let client = SimpleClient::new();
    let chain = Arc::new(client);

    // Instantiate the SimpleVerifier and SimpleBlockImport
    let verifier = SimpleVerifier;
    let block_import = SimpleBlockImport;

    // Create the import queue using BasicQueue
    let import_queue = Box::new(sc_consensus::BasicQueue::new(
        verifier,
        Box::new(block_import),
        None,                                   // Assuming no justification_import for simplicity
        &sp_core::testing::TaskExecutor::new(), // Adjust according to your async runtime
        None,                                   // Prometheus registry, if you have one
    ));

    let protocol_id = ProtocolId::from("test-protocol-name");

    let block_request_protocol_config = {
        let (handler, protocol_config) = sc_network::block_request_handler::BlockRequestHandler::new(&protocol_id, chain.clone(), 50);
        tokio::spawn(handler.run().boxed());
        protocol_config
    };

    let state_request_protocol_config = {
        let (handler, protocol_config) = sc_network::state_request_handler::StateRequestHandler::new(&protocol_id, chain.clone(), 50);
        tokio::spawn(handler.run().boxed());
        protocol_config
    };

    let light_client_request_protocol_config = {
        let (handler, protocol_config) = sc_network::light_client_requests::handler::LightClientRequestHandler::new(&protocol_id, chain.clone());
        tokio::spawn(handler.run().boxed());
        protocol_config
    };

    let network_params = sc_network::config::Params {
        role: Role::Light,
        executor: {
            Some(Box::new(|fut| {
                tokio::spawn(fut);
            }))
        },
        transactions_handler_executor: {
            Box::new(|fut| {
                tokio::spawn(fut);
            })
        },
        network_config,
        chain,
        transaction_pool: Arc::new(EmptyTransactionPool),
        protocol_id,
        import_queue,
        block_announce_validator: Box::new(sp_consensus::block_validation::DefaultBlockAnnounceValidator),
        metrics_registry: None,
        block_request_protocol_config,
        state_request_protocol_config,
        light_client_request_protocol_config,
        warp_sync: None,
    };

    let network = sc_network::NetworkWorker::new(network_params);

    tracing::info!("p2p server exiting");
    Ok(())
}

async fn get_network_config() -> anyhow::Result<NetworkConfiguration> {
    let mut network_config = NetworkConfiguration::new("test-node", "test-client", Default::default(), None);

    // Convert the network ID to a PeerId
    //XXX this is a temporary peer_id
    let peer_id = PeerId::from_str("12D3KooWEEShnkAbh9jKrJH5jbCRJQVLTpumYBvSoJDG8LgkyCKE")?;

    // Convert address to Multiaddr
    //XXX this is a temporary address
    let multiaddr: Multiaddr = "/dns4/p2p.testnet.cloudwalk.network/tcp/30333".parse()?;

    //TODO check those configurations
    network_config.sync_mode = SyncMode::Fast {
        skip_proofs: true,
        storage_chain_mode: false,
    };
    network_config.transport = TransportConfig::MemoryOnly;
    network_config.listen_addresses = vec![multiaddr.clone()];
    network_config.allow_non_globals_in_dht = true;

    let addrs = vec![MultiaddrWithPeerId { peer_id, multiaddr }];
    network_config.default_peers_set.reserved_nodes = addrs;
    network_config.default_peers_set.non_reserved_mode = NonReservedPeerMode::Deny;

    Ok(network_config)
}

#[derive(Clone, Debug)]
pub struct SimpleClient {}

impl SimpleClient {
    pub fn new() -> Self {
        SimpleClient {}
    }
}

// Your SimpleVerifier implementation
pub struct SimpleVerifier;
type CacheKeyId = [u8; 4];

#[async_trait::async_trait]
impl Verifier<Block> for SimpleVerifier {
    async fn verify(&mut self, block: BlockImportParams<Block, ()>) -> Result<(BlockImportParams<Block, ()>, Option<Vec<(CacheKeyId, Vec<u8>)>>), String> {
        info!("Verifying block: Number = {:?}, Hash = {:?}", block.header.number(), block.post_hash);
        Ok((block, None))
    }
}

// Your SimpleBlockImport implementation
pub struct SimpleBlockImport;

#[async_trait::async_trait]
impl BlockImport<Block> for SimpleBlockImport {
    type Error = ConsensusError;
    type Transaction = ();

    async fn check_block(&mut self, block: BlockCheckParams<Block>) -> Result<ImportResult, Self::Error> {
        info!("Checking block: Number = {:?}, Hash = {:?}", block.number, block.hash);
        Ok(ImportResult::imported(false))
    }

    async fn import_block(&mut self, block: BlockImportParams<Block, ()>, _cache: HashMap<CacheKeyId, Vec<u8>>) -> Result<ImportResult, Self::Error> {
        info!("Importing block: Number = {:?}, Hash = {:?}", block.header.number(), block.post_hash);
        Ok(ImportResult::imported(true))
    }
}

impl HeaderBackend<Block> for SimpleClient {
    fn info(&self) -> Info<Block> {
        info!("Called SimpleClient info()");
        Info {
            best_hash: Default::default(),
            best_number: 0,
            genesis_hash: Default::default(),
            finalized_hash: Default::default(),
            finalized_number: 0,
            finalized_state: None,
            number_leaves: 0,
            block_gap: None,
        }
    }

    fn header(&self, id: BlockId<Block>) -> Result<Option<sp_runtime::generic::Header<u64, BlakeTwo256>>, BlockchainError> {
        info!("Called SimpleClient header() with id: {:?}", id);
        Ok(None)
    }

    fn number(&self, hash: sp_core::H256) -> Result<Option<BlockNumber>, BlockchainError> {
        info!("Called SimpleClient number() with hash: {:?}", hash);
        Ok(None)
    }

    fn hash(&self, number: BlockNumber) -> Result<Option<sp_core::H256>, BlockchainError> {
        info!("Called SimpleClient hash() with number: {}", number);
        Ok(None)
    }

    fn status(&self, id: BlockId<Block>) -> Result<BlockStatus, BlockchainError> {
        info!("Called SimpleClient status() with id: {:?}", id);
        Ok(BlockStatus::Unknown) // Return a default status
    }
}

impl BlockBackend<Block> for SimpleClient {
    fn block_body(&self, id: &BlockId<Block>) -> sp_blockchain::Result<Option<Vec<Extrinsic>>> {
        info!("Called SimpleClient block_body() with id: {:?}", id);
        Ok(Some(Vec::new()))
    }

    fn block_indexed_body(&self, id: &BlockId<Block>) -> sp_blockchain::Result<Option<Vec<Vec<u8>>>> {
        info!("Called SimpleClient block_indexed_body() with id: {:?}", id);
        Ok(Some(Vec::new()))
    }

    fn block(&self, id: &BlockId<Block>) -> sp_blockchain::Result<Option<SignedBlock<Block>>> {
        info!("Called SimpleClient block() with id: {:?}", id);
        Ok(None)
    }

    fn block_status(&self, id: &BlockId<Block>) -> sp_blockchain::Result<sp_consensus::BlockStatus> {
        info!("Called SimpleClient block_status() with id: {:?}", id);
        Ok(sp_consensus::BlockStatus::Unknown)
    }

    fn justifications(&self, id: &BlockId<Block>) -> sp_blockchain::Result<Option<sp_runtime::Justifications>> {
        info!("Called SimpleClient justifications() with id: {:?}", id);
        Ok(None)
    }

    fn block_hash(&self, number: NumberFor<Block>) -> sp_blockchain::Result<Option<sp_core::H256>> {
        info!("Called SimpleClient block_hash() with number: {}", number);
        Ok(None)
    }

    fn indexed_transaction(&self, hash: &sp_core::H256) -> sp_blockchain::Result<Option<Vec<u8>>> {
        info!("Called SimpleClient indexed_transaction() with hash: {:?}", hash);
        Ok(None)
    }

    fn has_indexed_transaction(&self, hash: &sp_core::H256) -> sp_blockchain::Result<bool> {
        info!("Called SimpleClient has_indexed_transaction() with hash: {:?}", hash);
        Ok(false)
    }

    fn requires_full_sync(&self) -> bool {
        info!("Called SimpleClient requires_full_sync()");
        true
    }
}

impl ProofProvider<Block> for SimpleClient {
    fn read_proof(&self, id: &BlockId<Block>, keys: &mut dyn Iterator<Item = &[u8]>) -> sp_blockchain::Result<StorageProof> {
        info!("Called SimpleClient read_proof() with id: {:?}", id);
        Ok(StorageProof::new(Vec::new()))
    }

    fn read_child_proof(&self, id: &BlockId<Block>, child_info: &ChildInfo, keys: &mut dyn Iterator<Item = &[u8]>) -> sp_blockchain::Result<StorageProof> {
        info!("Called SimpleClient read_child_proof() with id: {:?} and child_info: {:?}", id, child_info);
        Ok(StorageProof::new(Vec::new()))
    }

    fn execution_proof(&self, id: &BlockId<Block>, method: &str, call_data: &[u8]) -> sp_blockchain::Result<(Vec<u8>, StorageProof)> {
        info!(
            "Called SimpleClient execution_proof() with id: {:?}, method: {}, call_data: {:?}",
            id, method, call_data
        );
        Ok((Vec::new(), StorageProof::new(Vec::new())))
    }

    fn read_proof_collection(&self, id: &BlockId<Block>, start_keys: &[Vec<u8>], size_limit: usize) -> sp_blockchain::Result<(CompactProof, u32)> {
        info!(
            "Called SimpleClient read_proof_collection() with id: {:?}, start_keys: {:?}, size_limit: {}",
            id, start_keys, size_limit
        );
        let compact_proof = CompactProof { encoded_nodes: Vec::new() };
        Ok((compact_proof, 0))
    }

    fn storage_collection(&self, id: &BlockId<Block>, start_key: &[Vec<u8>], size_limit: usize) -> sp_blockchain::Result<Vec<(KeyValueStorageLevel, bool)>> {
        info!(
            "Called SimpleClient storage_collection() with id: {:?}, start_key: {:?}, size_limit: {}",
            id, start_key, size_limit
        );
        Ok(Vec::new())
    }

    fn verify_range_proof(&self, root: sp_core::H256, proof: CompactProof, start_keys: &[Vec<u8>]) -> sp_blockchain::Result<(KeyValueStates, usize)> {
        info!(
            "Called SimpleClient verify_range_proof() with root: {:?}, proof: {:?}, start_keys: {:?}",
            root, proof, start_keys
        );
        Ok((KeyValueStates(vec![]), 0))
    }
}

use sp_blockchain::HeaderMetadata;
use sp_blockchain::CachedHeaderMetadata;

impl HeaderMetadata<Block> for SimpleClient {
    type Error = BlockchainError;

    fn header_metadata(&self, hash: sp_core::H256) -> Result<CachedHeaderMetadata<Block>, Self::Error> {
        info!("Called SimpleClient header_metadata() with hash: {:?}", hash);
        // Return a default or mock CachedHeaderMetadata
        let metadata = CachedHeaderMetadata {
            hash: Default::default(), // Dummy hash value
            number: 0,                // Dummy block number
            parent: Default::default(), // Dummy parent hash
            state_root: Default::default(), // Dummy state root hash
            ancestor: Default::default(), // Dummy ancestor hash
        };
        Ok(metadata)
    }

    fn insert_header_metadata(&self, hash: sp_core::H256, metadata: CachedHeaderMetadata<Block>) {
        info!("Called SimpleClient insert_header_metadata() with hash: {:?}, metadata: {:?}", hash, metadata);
        // Implement logic to insert header metadata
    }

    fn remove_header_metadata(&self, hash: sp_core::H256) {
        info!("Called SimpleClient remove_header_metadata() with hash: {:?}", hash);
        // Implement logic to remove header metadata
    }
}
