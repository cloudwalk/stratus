#!/bin/bash

set -e

# Function to build the project
build_project() {
    echo "Building the project..."
    cargo build --release --bin stratus --features metrics,rocks,dev
}

# Function to start an instance
start_instance() {
    local address=$1
    local grpc_address=$2
    local rocks_path_prefix=$3
    local log_file=$4
    local candidate_peers=$5
    local tokio_console_address=$6
    local metrics_exporter_address=$7

    RUST_LOG=info cargo run --features=metrics,rocks,dev --bin stratus -- \
        --enable-test-accounts \
        --candidate-peers="$candidate_peers" \
        -a=$address \
        --grpc-server-address=$grpc_address \
        --rocks-path-prefix=$rocks_path_prefix \
        --tokio-console-address=$tokio_console_address \
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
    local port=$1
    curl -s http://0.0.0.0:$port \
        --header "content-type: application/json" \
        --data '{"jsonrpc":"2.0","method":"consensus_isLeader","params":[],"id":1}' | jq '.result'
}

# Function to find the leader
find_leader() {
    local ports=("$@")
    local max_retries=10
    local retry_interval=5
    local retries=0
    while [ $retries -lt $max_retries ]; do
        for port in "${ports[@]}"; do
            result=$(check_leader $port)
            if [ "$result" = "true" ]; then
                echo $port
                return
            fi
        done
        retries=$((retries + 1))
        sleep $retry_interval
    done
    echo ""
}

# Function to remove rocks-path directory
remove_rocks_path() {
    local rocks_path=$1
    echo "Removing rocks-path directory $rocks_path"
    rm -rf $rocks_path
}

# Function to run the election test
run_test() {
    local num_instances=$1
    local instances=()

    # Prepare instance parameters
    for ((i=1; i<=num_instances; i++)); do
        local port=$((3000 + i))
        local grpc_port=$((3777 + i))
        local tokio_console_port=$((4000 + i))
        local metrics_exporter_port=$((5000 + i))
        local tmp_dir="tmp_rocks_$port"
        local log_file="instance_$port.log"

        # Generate candidate peers string
        candidate_peers=""
        for ((j=1; j<=num_instances; j++)); do
            if [ $i -ne $j ]; then
                if [ -n "$candidate_peers" ]; then
                    candidate_peers="$candidate_peers,"
                fi
                candidate_peers="$candidate_peers""http://0.0.0.0:$((3000 + j));$((3777 + j))"
            fi
        done

        instances+=("0.0.0.0:$port 0.0.0.0:$grpc_port $tmp_dir $log_file $port $candidate_peers 0.0.0.0:$tokio_console_port 0.0.0.0:$metrics_exporter_port")
    done

    # Start instances
    echo "Starting $num_instances instances..."
    pids=()
    ports=()
    rocks_paths=()
    readiness=()
    for instance in "${instances[@]}"; do
        IFS=' ' read -r -a params <<< "$instance"
        pids+=($(start_instance "${params[0]}" "${params[1]}" "${params[2]}" "${params[3]}" "${params[5]}" "${params[6]}" "${params[7]}"))
        ports+=("${params[4]}")
        rocks_paths+=("${params[2]}")
        readiness+=(false)
    done

    all_ready=false
    while [ "$all_ready" != true ]; do
        all_ready=true
        for i in "${!ports[@]}"; do
            if [ "${readiness[$i]}" != true ]; then
                response=$(check_liveness "${ports[$i]}")
                if [ "$response" = "true" ]; then
                    readiness[$i]=true
                    echo "Instance on port ${ports[$i]} is ready."
                else
                    all_ready=false
                fi
            fi
        done
        if [ "$all_ready" != true ]; then
            echo "Waiting for all instances to be ready..."
            sleep 5 # Wait for a bit before checking again
        fi
    done

    echo "All instances are ready."

    # Check which instance is the leader
    leader_port=$(find_leader "${ports[@]}")
    if [ -n "$leader_port" ]; then
        echo "Leader found on port $leader_port"

        # Kill the leader instance
        echo "Killing the leader instance on port $leader_port..."
        for i in "${!ports[@]}"; do
            if [ "${ports[i]}" -eq "$leader_port" ]; then
                kill ${pids[i]}
                unset ports[i]
                unset pids[i]
                break
            fi
        done

        sleep 5

        # Restart the killed instance
        echo "Restarting the killed instance..."
        for instance in "${instances[@]}"; do
            IFS=' ' read -r -a params <<< "$instance"
            if [ "${params[4]}" -eq "$leader_port" ]; then
                new_pid=$(start_instance "${params[0]}" "${params[1]}" "${params[2]}" "${params[3]}" "${params[5]}" "${params[6]}" "${params[7]}")
                pids+=("$new_pid")
                ports+=("${params[4]}")
                readiness+=(false)
                break
            fi
        done

        all_ready=false
        while [ "$all_ready" != true ]; do
            all_ready=true
            for i in "${!ports[@]}"; do
                if [ "${readiness[$i]}" != true ]; then
                    response=$(check_liveness "${ports[$i]}")
                    if [ "$response" = "true" ]; then
                        readiness[$i]=true
                        echo "Instance on port ${ports[$i]} is ready."
                    else
                        all_ready=false
                    fi
                fi
            done
            if [ "$all_ready" != true ]; then
                echo "Waiting for all instances to be ready..."
                sleep 5 # Wait for a bit before checking again
            fi
        done

        echo "All instances are ready after restart."

        # Maximum timeout duration in seconds for new leader election
        max_timeout=30

        # Capture the start time
        start_time=$(date +%s)

        # Wait until a new leader is found or timeout
        while true; do
            current_time=$(date +%s)
            elapsed_time=$((current_time - start_time))

            if [ $elapsed_time -ge $max_timeout ]; then
                echo "Timeout reached without finding a new leader."
                break # Exit the loop due to timeout
            fi

            new_leader_port=$(find_leader "${ports[@]}")
            if [ -n "$new_leader_port" ]; then
                echo "New leader found on port $new_leader_port"
                break # Exit the loop since a new leader has been found
            else
                echo "Waiting for a new leader to be elected..."
                sleep 5
            fi
        done
    else
        echo "Error: No leader found" >&2
        exit 1
    fi

    # Clean up
    echo "Cleaning up instances..."
    for pid in "${pids[@]}"; do
        kill $pid
    done

    for rocks_path in "${rocks_paths[@]}"; do
        remove_rocks_path $rocks_path
    done
}

# Number of instances
num_instances=3

# Number of times to run the test
n=10

# Build the project
build_project

# Run the test n times
for ((iteration_n=1; iteration_n<=n; iteration_n++)); do
    echo -e "\n##############################################\n"
    echo "Running test iteration $iteration_n of $n..."
    run_test $num_instances
    sleep 5
done
