use sc_network::behaviour::Behaviour;
use sc_network::config::{Role, SyncMode};
use sc_network::bitswap::Bitswap;
use sc_network::Multiaddr;
use sc_network::PeerId;
use libp2p::swarm::{Swarm, SwarmBuilder};
use sc_network::config::NonReservedPeerMode;
use sc_network::config::MultiaddrWithPeerId;
use sc_network::config::TransportConfig;
use sc_network::config::NetworkConfiguration;
use std::str::FromStr;

pub async fn serve_p2p() -> anyhow::Result<()> {
    let _ = get_full_network_config().await;
    tracing::info!("connecting to peers");

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

// fn build_params() -> sc_network::config::Params {
//     let network_params = sc_network::config::Params {
//         role: Role::Discover,
//         executor: {
//             Box::new(|fut| {
//                 tokio::spawn(fut);
//             })
//         },
//         network_config: get_full_network_config(),
//         peer_store: peer_store_handle,
//         genesis_hash,
//         protocol_id: protocol_id.clone(),
//         fork_id: config.chain_spec.fork_id().map(ToOwned::to_owned),
//         metrics_registry: config.prometheus_config.as_ref().map(|config| config.registry.clone()),
//         block_announce_config,
//         tx,
//     };
//
//     network_params
// }

async fn get_full_network_config() -> anyhow::Result<NetworkConfiguration> {
    let mut network_config =
        NetworkConfiguration::new("test-node", "test-client", Default::default(), None);

    // Convert the network ID to a PeerId
    let peer_id = PeerId::from_str("12D3KooWEEShnkAbh9jKrJH5jbCRJQVLTpumYBvSoJDG8LgkyCKE")?;

    // Convert address to Multiaddr
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
