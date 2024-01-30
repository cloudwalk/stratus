use sc_network::behaviour::Behaviour;
use sc_network::config::{Role, SyncMode, EmptyTransactionPool, ProtocolId};
use sc_network::bitswap::Bitswap;
use sc_network::Multiaddr;
use sc_network::PeerId;
use libp2p::swarm::{Swarm, SwarmBuilder};
use sc_network::config::NonReservedPeerMode;
use sc_network::config::MultiaddrWithPeerId;
use sc_network::config::TransportConfig;
use sc_network::config::NetworkConfiguration;
use std::str::FromStr;
use std::sync::Arc;

pub async fn serve_p2p() -> anyhow::Result<()> {
    tracing::info!("connecting to peers");

    let network_config = get_network_config().await?;
    let client = SimpleClient::new();
    let chain = Arc::new(client);

    //let network_params = sc_network::config::Params {
    //    role: Role::Light,
    //    executor: {
    //        Some(Box::new(|fut| {
    //            tokio::spawn(fut);
    //        }))
    //    },
    //    transactions_handler_executor: {
    //        Box::new(|fut| {
    //            tokio::spawn(fut);
    //        })
    //    },
    //    network_config,
    //    chain,
    //    transaction_pool: Arc::new(EmptyTransactionPool),
    //    protocol_id: ProtocolId::from("test-protocol-name"),
    //    import_queue,
    //    block_announce_validator: config
    //        .block_announce_validator
    //        .unwrap_or_else(|| Box::new(DefaultBlockAnnounceValidator)),
    //    metrics_registry: None,
    //    block_request_protocol_config,
    //    state_request_protocol_config,
    //    light_client_request_protocol_config,
    //    warp_sync: Some((warp_sync, warp_protocol_config)),
    //};

    Ok(())

    //XXX let params = build_params();
    //
    //    let client = params.chain.clone();
    //    let behaviour = {
    //        let bitswap = params.network_config.ipfs_server.then(|| Bitswap::new(client));
    //        let result = Behaviour::new(
    //            protocol,
    //            user_agent,
    //            local_public,
    //            discovery_config,
    //            params.block_request_protocol_config,
    //            params.state_request_protocol_config,
    //            warp_sync_protocol_config,
    //            bitswap,
    //            params.light_client_request_protocol_config,
    //            params.network_config.request_response_protocols,
    //            peerset_handle.clone(),
    //        );
    //
    //        match result {
    //            Ok(b) => b,
    //            Err(crate::request_responses::RegisterError::DuplicateProtocol(proto)) =>
    //                return Err(Error::DuplicateRequestResponseProtocol { protocol: proto }),
    //        }
    //    };
}

async fn get_network_config() -> anyhow::Result<NetworkConfiguration> {
    let mut network_config =
        NetworkConfiguration::new("test-node", "test-client", Default::default(), None);

    // Convert the network ID to a PeerId
    //XXX this is a temporary peer_id
    let peer_id = PeerId::from_str("12D3KooWEEShnkAbh9jKrJH5jbCRJQVLTpumYBvSoJDG8LgkyCKE")?;

    // Convert address to Multiaddr
    //XXX this is a temporary address
    let multiaddr: Multiaddr = "/dns4/p2p.testnet.cloudwalk.network/tcp/30333".parse()?;

    //TODO check those configurations
    network_config.sync_mode = SyncMode::Fast { skip_proofs: true, storage_chain_mode: false };
    network_config.transport = TransportConfig::MemoryOnly;
    network_config.listen_addresses = vec![multiaddr.clone()];
    network_config.allow_non_globals_in_dht = true;

    let addrs = vec![MultiaddrWithPeerId { peer_id, multiaddr }];
    network_config.default_peers_set.reserved_nodes = addrs;
    network_config.default_peers_set.non_reserved_mode = NonReservedPeerMode::Deny;

    Ok(network_config)
}

pub struct SimpleClient {
}

impl SimpleClient {
    pub fn new() -> Self {
        SimpleClient {
        }
    }
}
