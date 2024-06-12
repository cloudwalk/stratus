#!/bin/bash

# Function to check the leader
check_leader() {
    stable=0
    while [ $stable -ne 1 ]; do
        leaders=0
        leader_pod=""
        for pod in $(kubectl get pods -l app=stratus-api -o jsonpath='{.items[*].metadata.name}'); do
            role=$(kubectl exec -it $pod -- curl -s http://0.0.0.0:3000 --header "content-type: application/json" --data '{"jsonrpc":"2.0","method":"consensus_isLeader","params":[],"id":1}' | jq -r .result)
            if [ "$role" = "true" ]; then
                leaders=$((leaders + 1))
                leader_pod=$pod
            fi
        done
        if [ $leaders -eq 1 ]; then
            stable=1
        elif [ $leaders -gt 1 ]; then
            echo "Error: More than one leader elected."
            exit 1
        else
            echo "Waiting for a single leader..."
            sleep 5
        fi
    done
    echo "Leader found: $leader_pod"
    leader_pod_name=$leader_pod
}

# Function to kill random pods
kill_random_pods() {
    total_pods=$(kubectl get pods -l app=stratus-api -o jsonpath='{.items[*].metadata.name}' | wc -w)
    if [ $total_pods -gt 0 ]; then
        # Select a random number of pods to kill, from 1 to total_pods
        num_to_kill=$((RANDOM % total_pods + 1))
        pods_to_kill=$(kubectl get pods -l app=stratus-api -o jsonpath='{.items[*].metadata.name}' | tr ' ' '\n' | shuf | head -n $num_to_kill)
        for pod in $pods_to_kill; do
            echo "Killing pod: $pod"
            kubectl delete pod $pod
        done
    else
        echo "No pods found, nothing to kill."
    fi
}

# Function to wait for a new leader with timeout
wait_for_new_leader() {
    timeout=${1:-60}  # Default timeout is 60 seconds
    echo "Waiting for new leader to be elected with a timeout of $timeout seconds..."
    stable=0
    start_time=$(date +%s)
    while [ $stable -ne 1 ]; do
        leaders=0
        for pod in $(kubectl get pods -l app=stratus-api -o jsonpath='{.items[*].metadata.name}'); do
            role=$(kubectl exec -it $pod -- curl -s http://0.0.0.0:3000 --header "content-type: application/json" --data '{"jsonrpc":"2.0","method":"consensus_isLeader","params":[],"id":1}' | jq -r .result)
            if [ "$role" = "true" ]; then
                leaders=$((leaders + 1))
            fi
        done
        if [ $leaders -eq 1 ]; then
            stable=1
        elif [ $leaders -gt 1 ]; then
            echo "Error: More than one leader elected."
            exit 1
        else
            echo "Waiting for new leader..."
            sleep 5
        fi
        current_time=$(date +%s)
        elapsed_time=$((current_time - start_time))
        if [ $elapsed_time -ge $timeout ]; then
            echo "Error: Timeout reached while waiting for new leader."
            exit 1
        fi
    done
    echo "New leader elected."
}

# Main script
repeat_count=${1:-5}   # Number of times to repeat the process, default is 5
timeout=${2:-60}       # Timeout for waiting for new leader, default is 60 seconds

for i in $(seq 1 $repeat_count); do
    echo "Starting election process iteration $i"
    check_leader
    kill_random_pods
    wait_for_new_leader $timeout
    echo "Completed election process iteration $i"
done

echo "Election process completed $repeat_count times."
