use std::str::FromStr;
use std::sync::Arc;

use sc_network::config::MultiaddrWithPeerId;
use sc_network::config::NetworkConfiguration;
use sc_network::config::NonReservedPeerMode;
use sc_network::config::SyncMode;
use sc_network::config::TransportConfig;
use sc_network::Multiaddr;
use sc_network::PeerId;

pub async fn serve_p2p() -> anyhow::Result<()> {
    tracing::info!("connecting to peers");

    let _network_config = get_network_config().await?;
    let client = SimpleClient::new();
    let _chain = Arc::new(client);

    //HERE

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
