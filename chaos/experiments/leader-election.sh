#!/bin/bash

set -e

# Default binary
binary="stratus"

# Default number of instances
num_instances=3

# Default number of iterations
iterations=4

# Flag for enabling leader restart feature
enable_leader_restart=false

# Parse command-line options
while [[ "$#" -gt 0 ]]; do
  case $1 in
    --bin)
      binary="$2"
      shift 2
      ;;
    --instances)
      num_instances="$2"
      shift 2
      ;;
    --iterations)
      iterations="$2"
      shift 2
      ;;
    --enable-leader-restart)
      if [[ "$2" == "true" || "$2" == "false" ]]; then
        enable_leader_restart="$2"
        shift 2
      else
        echo "Error: --enable-leader-restart must be followed by true or false"
        exit 1
      fi
      ;;
    *)
      echo "Unknown parameter passed: $1"
      echo "Usage: $0 [--bin binary] [--instances number] [--iterations number] [--enable-leader-restart true|false]"
      exit 1
      ;;
  esac
done

echo "Using binary: $binary"
echo "Number of instances: $num_instances"
echo "Number of iterations: $iterations"
echo "Enable leader restart: $enable_leader_restart"

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
    else
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
    find . -type d -name "tmp_rocks_*" -print0 | xargs -0 rm -rf
}

# Function to run the election test
run_test() {
    local instances=()
    local all_addresses=()

    for ((i=1; i<=num_instances; i++)); do
        local base_port=$((3000 + i))
        local grpc_port=$((3777 + i))
        all_addresses+=("http://0.0.0.0:$base_port;$grpc_port")
    done

    for ((i=1; i<=num_instances; i++)); do
        local base_port=$((3000 + i))
        local grpc_port=$((3777 + i))
        local tokio_console_port=$((6668 + i))
        local metrics_exporter_port=$((9000 + i))
        
        # Exclude current instance's address to get candidate_peers
        local candidate_peers=($(printf "%s\n" "${all_addresses[@]}" | grep -v "http://0.0.0.0:$base_port;$grpc_port"))
        local candidate_peers_str=""

        if [ ${#candidate_peers[@]} -gt 0 ]; then
            candidate_peers_str=$(printf ",%s" "${candidate_peers[@]}")
            candidate_peers_str=${candidate_peers_str:1}
        fi
        instances+=("0.0.0.0:$base_port 0.0.0.0:$grpc_port tmp_rocks_$base_port instance_$base_port.log $base_port ${candidate_peers_str} 0.0.0.0:$tokio_console_port 0.0.0.0:$metrics_exporter_port")
    done

    # Start instances
    echo "Starting $num_instances instance(s)..."
    ports=()
    grpc_addresses=()
    rocks_paths=()
    liveness=()
    for instance in "${instances[@]}"; do
        IFS=' ' read -r -a params <<< "$instance"
        start_instance "${params[0]}" "${params[1]}" "${params[2]}" "${params[3]}" "${params[5]}" "${params[6]}" "${params[7]}"
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

    if [ "$enable_leader_restart" = true ]; then
        # Kill the leader instance
        echo "Killing the leader instance on address $leader_grpc_address..."
        for i in "${!grpc_addresses[@]}"; do
            if [ "${grpc_addresses[i]}" == "$leader_grpc_address" ]; then
                killport --quiet ${ports[i]}
                break
            fi
        done

        # Restart the killed instance
        echo "Restarting the killed instance..."
        for i in "${!instances[@]}"; do
            IFS=' ' read -r -a params <<< "${instances[i]}"
            if [ "${params[1]}" == "$leader_grpc_address" ]; then
                start_instance "${params[0]}" "${params[1]}" "${params[2]}" "${params[3]}" "${params[5]}" "${params[6]}" "${params[7]}"
                liveness[i]=false
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
        sleep 15 # wait until election is settled down

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
    fi

    # Clean up
    echo "Cleaning up..."
    for port in "${ports[@]}"; do
        killport --quiet $port
    done
    
    remove_rocks_path
}

# Run the test n times
for ((iteration_n=1; iteration_n<=$iterations; iteration_n++)); do
    echo -e "\n##############################################\n"
    echo "Running binary $binary test iteration $iteration_n of $iterations..."
    run_test
    sleep 5
done
