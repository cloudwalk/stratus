use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use sc_consensus::BlockCheckParams;
use sc_consensus::BlockImport;
use sc_consensus::BlockImportParams;
use sc_consensus::ImportResult;
use sc_consensus::Verifier;
use sc_network::config::MultiaddrWithPeerId;
use sc_network::config::NetworkConfiguration;
use sc_network::config::NonReservedPeerMode;
use sc_network::config::SyncMode;
use sc_network::config::TransportConfig;
use sc_network::Multiaddr;
use sc_network::PeerId;
use sp_consensus::error::Error as ConsensusError;
use sp_runtime::traits::Block as BlockT;
use sp_runtime::traits::Header as HeaderT;
use tracing::info;
use codec::{Decode, Encode};

use sp_runtime::traits::{BlakeTwo256, Extrinsic as ExtrinsicT, Verify};
use sp_core::RuntimeDebug;

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

    let _network_config = get_network_config().await?;
    let client = SimpleClient::new();
    let _chain = Arc::new(client);

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
