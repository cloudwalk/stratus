#!/bin/bash

check_leader() {
    local grpc_address=$1

    # Send the gRPC request using grpcurl and capture both stdout and stderr
    response=$(grpcurl -import-path static/proto -proto append_entry.proto -plaintext -d '{"term": 0, "prevLogIndex": 0, "prevLogTerm": 0, "leader_id": "leader_id_value", "block_entry": {"number": 1, "hash": "hash_value", "transactions_root": "transactions_root_value", "gas_used": "gas_used_value", "gas_limit": "gas_limit_value", "bloom": "bloom_value", "timestamp": 123456789, "parent_hash": "parent_hash_value", "author": "author_value", "extra_data": "ZXh0cmFfZGF0YV92YWx1ZQ==", "miner": "miner_value", "difficulty": "difficulty_value", "receipts_root": "receipts_root_value", "uncle_hash": "uncle_hash_value", "size": 12345, "state_root": "state_root_value", "total_difficulty": "total_difficulty_value", "nonce": "nonce_value", "transaction_hashes": ["tx_hash1", "tx_hash2"]}}' "$grpc_address" append_entry.AppendEntryService/AppendBlockCommit 2>&1)

    # Check the response for specific strings to determine the node status
    if [[ "$response" == *"append_transaction_executions called on leader node"* ]]; then
        return 0 # Success exit code for leader
    elif [[ "$response" == *"APPEND_SUCCESS"* ]]; then
        return 1 # Failure exit code for non-leader
    else
        echo "Unexpected response from $grpc_address: $response"
        exit 1
    fi
}

find_leader() {
    local grpc_addresses=("$@")
    local leaders=()
    for grpc_address in "${grpc_addresses[@]}"; do
        if check_leader "$grpc_address"; then
            leaders+=("$grpc_address")
        fi
    done
    if [ ${#leaders[@]} -eq 0 ]; then
        echo "No leader nodes found."
    else
        echo "Leader(s): ${leaders[@]}"
    fi
}

# Example usage with provided gRPC addresses
grpc_addresses=("0.0.0.0:3778" "0.0.0.0:3779")
find_leader "${grpc_addresses[@]}"