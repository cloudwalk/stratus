/// This module is responsible for discovering peers in the network.
/// It will then attempt to connect to the candidate peers and add them to the consensus module.
/// It will also remove any peers that are no longer available.
/// It also spawn a new task to handle the block propagation to the new peer.
use std::sync::Arc;

#[cfg(not(test))]
use tokio::sync::Mutex;

#[cfg(not(test))]
use super::append_entry::append_entry_service_client::AppendEntryServiceClient;
use super::Consensus;
use super::Peer;
use super::PeerAddress;
#[cfg(not(test))]
use super::Role;
use crate::ext::spawn_named;
#[cfg(feature = "metrics")]
use crate::infra::metrics;

#[tracing::instrument(skip_all)]
pub async fn discover_peers(consensus: Arc<Consensus>) {
    #[allow(unused_mut)]
    let mut new_peers: Vec<(PeerAddress, Peer)> = Vec::new();

    #[cfg(not(test))] // FIXME: This is a workaround to avoid running this code in tests we need a proper Tonic mock
    match discover_peers_env(&consensus.direct_peers, Arc::clone(&consensus)).await {
        Ok(env_peers) => {
            tracing::info!("discovered {} peers from env", env_peers.len());
            new_peers.extend(env_peers);
        }
        Err(e) => {
            tracing::warn!("failed to discover peers from env: {:?}", e);
        }
    }

    let mut peers_lock = consensus.peers.write().await;

    // Collect current peer addresses
    let current_addresses: Vec<PeerAddress> = peers_lock.keys().cloned().collect();
    let discovered_addresses: Vec<PeerAddress> = new_peers.iter().map(|(addr, _)| addr.clone()).collect();

    // Purge old peers
    let purged_addresses: Vec<PeerAddress> = current_addresses.into_iter().filter(|addr| !discovered_addresses.contains(addr)).collect();

    for address in &purged_addresses {
        peers_lock.remove(address);
    }

    tracing::info!(
        purged_peers = purged_addresses.iter().map(|p| p.to_string()).collect::<Vec<String>>().join(", "),
        "purged old peers",
    );

    for (address, peer) in new_peers {
        if peers_lock.contains_key(&address) {
            tracing::info!("consensus module peer {} already exists, skipping initialization", address);
            continue;
        }

        let consensus_clone = Arc::clone(&consensus);
        let peer_clone = peer.clone();

        let handle = spawn_named("consensus::propagate", async move {
            super::Consensus::handle_peer_propagation(peer_clone, consensus_clone).await;
        });

        tracing::info!("consensus module adding new peer: {}", address.address);
        peers_lock.insert(address, (peer, handle));
    }

    #[cfg(feature = "metrics")]
    metrics::set_consensus_available_peers(peers_lock.len() as u64);

    tracing::info!(
        peers = peers_lock.keys().map(|p| p.to_string()).collect::<Vec<String>>().join(", "),
        "consensus module discovered peers",
    );
}

#[cfg(not(test))] // FIXME: This is a workaround to avoid running this code in tests we need a proper Tonic mock
async fn discover_peers_env(addresses: &[String], consensus: Arc<Consensus>) -> Result<Vec<(PeerAddress, Peer)>, anyhow::Error> {
    #[allow(unused_mut)]
    let mut peers: Vec<(PeerAddress, Peer)> = Vec::new();

    for address in addresses {
        match PeerAddress::from_string(address.to_string()) {
            Ok(peer_address) => {
                if peer_address == consensus.my_address
                    || peer_address == PeerAddress::new("http://0.0.0.0".to_string(), consensus.my_address.jsonrpc_port, consensus.my_address.grpc_port)
                {
                    continue;
                }

                let grpc_address = peer_address.full_grpc_address();
                tracing::info!("Attempting to connect to peer gRPC address: {}", grpc_address);
                match AppendEntryServiceClient::connect(grpc_address.clone()).await {
                    Ok(client) => {
                        tracing::info!("Successfully connected to peer gRPC address: {}", grpc_address);
                        let peer = Peer {
                            client,
                            match_index: 0,
                            next_index: 0,
                            role: Role::Follower, // FIXME it won't be always follower, we need to check the leader or candidates
                            receiver: Arc::new(Mutex::new(consensus.broadcast_sender.subscribe())),
                        };
                        peers.push((peer_address.clone(), peer));
                        tracing::info!(peer = peer_address.to_string(), "peer is available");
                    }
                    Err(e) => {
                        tracing::warn!(peer = peer_address.to_string(), "peer is not available. Error: {:?}", e);
                    }
                }
            }
            Err(e) => {
                tracing::error!("Invalid address format: {}. Error: {:?}", address, e);
            }
        }
    }

    tracing::info!("Completed peer discovery with {} peers found", peers.len());
    Ok(peers)
}
