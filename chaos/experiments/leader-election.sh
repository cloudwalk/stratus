#!/bin/bash

set -e

# Default binary
binary="stratus"

# Parse command-line options
while [[ "$#" -gt 0 ]]; do
  case $1 in
    --bin)
      binary="$2"
      shift 2
      ;;
    *)
      echo "Unknown parameter passed: $1"
      echo "Usage: $0 [--bin binary]"
      exit 1
      ;;
  esac
done

echo "Using binary: $binary"

# Function to start an instance
start_instance() {
    local address=$1
    local grpc_address=$2
    local rocks_path_prefix=$3
    local log_file=$4
    local candidate_peers=$5
    local tokio_console_address=$6
    local metrics_exporter_address=$7

    RUST_LOG=info cargo run --release --bin $binary --features dev -- \
        --block-mode=1s \
        --enable-test-accounts \
        --candidate-peers="$candidate_peers" \
        -a=$address \
        --grpc-server-address=$grpc_address \
        --rocks-path-prefix=$rocks_path_prefix \
        --tokio-console-address=$tokio_console_address \
        --perm-storage=rocks \
        --metrics-exporter-address=$metrics_exporter_address > $log_file 2>&1 &
    echo $!
}

# Function to check liveness of an instance
check_liveness() {
    local port=$1
    curl -s http://0.0.0.0:$port \
        --header "content-type: application/json" \
        --data '{"jsonrpc":"2.0","method":"stratus_liveness","params":[],"id":1}' | jq '.result'
}

# Function to check if an instance is the leader
check_leader() {
    local grpc_address=$1

    # Send the gRPC request using grpcurl and capture both stdout and stderr
    response=$(grpcurl -import-path static/proto -proto append_entry.proto -plaintext -d '{"term": 0, "prevLogIndex": 0, "prevLogTerm": 0, "leader_id": "leader_id_value", "block_entry": {"number": 1, "hash": "hash_value", "transactions_root": "transactions_root_value", "gas_used": "gas_used_value", "gas_limit": "gas_limit_value", "bloom": "bloom_value", "timestamp": 123456789, "parent_hash": "parent_hash_value", "author": "author_value", "extra_data": "ZXh0cmFfZGF0YV92YWx1ZQ==", "miner": "miner_value", "difficulty": "difficulty_value", "receipts_root": "receipts_root_value", "uncle_hash": "uncle_hash_value", "size": 12345, "state_root": "state_root_value", "total_difficulty": "total_difficulty_value", "nonce": "nonce_value", "transaction_hashes": ["tx_hash1", "tx_hash2"]}}' "$grpc_address" append_entry.AppendEntryService/AppendBlockCommit 2>&1)

    # Check the response for specific strings to determine the node status
    if [[ "$response" == *"append_transaction_executions called on leader node"* ]]; then
        return 0 # Success exit code for leader
    elif [[ "$response" == *"APPEND_SUCCESS"* ]]; then
        return 1 # Failure exit code for non-leader
    fi
}

# Function to find the leader node
find_leader() {
    local grpc_addresses=("$@")
    local leaders=()
    for grpc_address in "${grpc_addresses[@]}"; do
        check_leader "$grpc_address"
        local status=$?
        if [ $status -eq 0 ]; then
            leaders+=("$grpc_address")
        fi
    done
    
    echo "${leaders[@]}"
}

# Function to remove rocks-path directory
remove_rocks_path() {
    local rocks_path=$1
    rm -rf data/$rocks_path
}

# Function to run the election test
run_test() {
    local instances=(
        "0.0.0.0:3001 0.0.0.0:3778 tmp_rocks_3001 instance_3001.log 3001 http://0.0.0.0:3002;3779,http://0.0.0.0:3003;3780 0.0.0.0:6669 0.0.0.0:9001"
        "0.0.0.0:3002 0.0.0.0:3779 tmp_rocks_3002 instance_3002.log 3002 http://0.0.0.0:3001;3778,http://0.0.0.0:3003;3780 0.0.0.0:6670 0.0.0.0:9002"
        "0.0.0.0:3003 0.0.0.0:3780 tmp_rocks_3003 instance_3003.log 3003 http://0.0.0.0:3001;3778,http://0.0.0.0:3002;3779 0.0.0.0:6671 0.0.0.0:9003"
    )

    # Start instances
    echo "Starting 3 instances..."
    pids=()
    ports=()
    grpc_addresses=()
    rocks_paths=()
    liveness=()
    for instance in "${instances[@]}"; do
        IFS=' ' read -r -a params <<< "$instance"
        pids+=($(start_instance "${params[0]}" "${params[1]}" "${params[2]}" "${params[3]}" "${params[5]}" "${params[6]}" "${params[7]}"))
        ports+=("${params[4]}")
        grpc_addresses+=("${params[1]}")
        rocks_paths+=("${params[2]}")
        liveness+=(false)
    done

    all_ready=false
    while [ "$all_ready" != true ]; do
        all_ready=true
        for i in "${!ports[@]}"; do
            if [ "${liveness[$i]}" != true ]; then
                response=$(check_liveness "${ports[$i]}")
                if [ "$response" = "true" ]; then
                    liveness[$i]=true
                    echo "Instance on port ${ports[$i]} is ready."
                else
                    all_ready=false
                fi
            fi
        done
        if [ "$all_ready" != true ]; then
            echo "Waiting for all instances to be ready..."
            sleep 5
        fi
    done

    echo "All instances are ready. Waiting for leader election"

    # Maximum timeout duration in seconds for the initial leader election
    initial_leader_timeout=60

    # Capture the start time
    initial_start_time=$(date +%s)

    # Wait for the initial leader election with a timeout
    while true; do
        current_time=$(date +%s)
        elapsed_time=$((current_time - initial_start_time))

        if [ $elapsed_time -ge $initial_leader_timeout ]; then
            echo "Timeout reached without finding an initial leader."
            exit 1
        fi

        leader_grpc_addresses=($(find_leader "${grpc_addresses[@]}"))
        if [ ${#leader_grpc_addresses[@]} -gt 1 ]; then
            echo "Error: More than one leader found: ${leader_grpc_addresses[*]}"
            exit 1
        elif [ ${#leader_grpc_addresses[@]} -eq 1 ]; then
            leader_grpc_address=${leader_grpc_addresses[0]}
            echo "Leader found on address $leader_grpc_address"
            break
        else
            sleep 1
        fi
    done

    if [ -z "$leader_grpc_address" ]; then
        echo "Exiting due to leader election failure."
        exit 1
    fi

    # Kill the leader instance
    echo "Killing the leader instance on address $leader_grpc_address..."
    for i in "${!grpc_addresses[@]}"; do
        if [ "${grpc_addresses[i]}" == "$leader_grpc_address" ]; then
            kill ${pids[i]}
            unset grpc_addresses[i]
            unset pids[i]
            unset ports[i]
            break
        fi
    done

    # Wait some time for the instance to be killed completely
    sleep 15

    # Restart the killed instance
    echo "Restarting the killed instance..."
    for instance in "${instances[@]}"; do
        IFS=' ' read -r -a params <<< "$instance"
        if [ "${params[1]}" == "$leader_grpc_address" ]; then
            new_pid=$(start_instance "${params[0]}" "${params[1]}" "${params[2]}" "${params[3]}" "${params[5]}" "${params[6]}" "${params[7]}")
            pids+=("$new_pid")
            grpc_addresses+=("${params[1]}")
            ports+=("${params[4]}")
            liveness+=(false)
            break
        fi
    done

    restart_all_ready=false
    while [ "$restart_all_ready" != true ]; do
        restart_all_ready=true
        for i in "${!ports[@]}"; do
            if [ "${liveness[$i]}" != true ]; then
                response=$(check_liveness "${ports[$i]}")
                if [ "$response" = "true" ]; then
                    liveness[$i]=true
                    echo "Instance on address ${ports[$i]} is ready."
                else
                    restart_all_ready=false
                fi
            fi
        done
        if [ "$restart_all_ready" != true ]; then
            echo "Waiting for all instances to be ready..."
            sleep 5
        fi
    done

    echo "All instances are ready after restart. Waiting for new leader election."

    # Maximum timeout duration in seconds for new leader election
    max_timeout=60

    # Capture the start time
    start_time=$(date +%s)

    # Wait until a new leader is found or timeout
    while true; do
        current_time=$(date +%s)
        elapsed_time=$((current_time - start_time))

        if [ $elapsed_time -ge $max_timeout ]; then
            echo "Timeout reached without finding a new leader."
            exit 1
        fi

        leader_grpc_addresses=($(find_leader "${grpc_addresses[@]}"))
        if [ ${#leader_grpc_addresses[@]} -gt 1 ]; then
            echo "Error: More than one leader found: ${leader_grpc_addresses[*]}"
            exit 1
        elif [ ${#leader_grpc_addresses[@]} -eq 1 ]; then
            leader_grpc_address=${leader_grpc_addresses[0]}
            echo "Leader found on address $leader_grpc_address"
            break
        else
            sleep 1
        fi
    done

    # Clean up
    echo "Cleaning up..."
    for pid in "${pids[@]}"; do
        kill $pid
    done

    for rocks_path in "${rocks_paths[@]}"; do
        remove_rocks_path $rocks_path
    done
}

# Number of times to run the test
n=4

# Run the test n times
for ((iteration_n=1; iteration_n<=n; iteration_n++)); do
    echo -e "\n##############################################\n"
    echo "Running binary $binary test iteration $iteration_n of $n..."
    run_test
    sleep 5
done
