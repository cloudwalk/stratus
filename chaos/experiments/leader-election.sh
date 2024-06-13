#!/bin/bash

set -e

# Function to start an instance
start_instance() {
    local address=$1
    local grpc_address=$2
    local rocks_path_prefix=$3
    local log_file=$4
    local candidate_peers=$5
    local tokio_console_address=$6
    local metrics_exporter_address=$7

    RUST_LOG=info cargo run --release --bin stratus --features dev -- \
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
    local leaders=()
    for port in "${ports[@]}"; do
        result=$(check_leader $port)
        if [ "$result" = "true" ]; then
            leaders+=($port)
        fi
    done
    echo "${leaders[@]}"
}

# Function to remove rocks-path directory
remove_rocks_path() {
    local rocks_path=$1
    rm -rf $rocks_path
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
    rocks_paths=()
    liveness=()
    for instance in "${instances[@]}"; do
        IFS=' ' read -r -a params <<< "$instance"
        pids+=($(start_instance "${params[0]}" "${params[1]}" "${params[2]}" "${params[3]}" "${params[5]}" "${params[6]}" "${params[7]}"))
        ports+=("${params[4]}")
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

        leader_ports=($(find_leader "${ports[@]}"))
        if [ ${#leader_ports[@]} -gt 1 ]; then
            echo "Error: More than one leader found: ${leader_ports[*]}"
            exit 1
        elif [ ${#leader_ports[@]} -eq 1 ]; then
            leader_port=${leader_ports[0]}
            echo "Leader found on port $leader_port"
            break
        else
            sleep 1
        fi
    done

    if [ -z "$leader_port" ]; then
        echo "Exiting due to leader election failure."
        exit 1
    fi

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
            liveness+=(false)
            break
        fi
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

        leader_ports=($(find_leader "${ports[@]}"))
        if [ ${#leader_ports[@]} -gt 1 ]; then
            echo "Error: More than one leader found: ${leader_ports[*]}"
            exit 1
        elif [ ${#leader_ports[@]} -eq 1 ]; then
            leader_port=${leader_ports[0]}
            echo "Leader found on port $leader_port"
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
n=5

# Run the test n times
for ((iteration_n=1; iteration_n<=n; iteration_n++)); do
    echo -e "\n##############################################\n"
    echo "Running test iteration $iteration_n of $n..."
    run_test
    sleep 5
done
