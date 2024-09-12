#!/bin/bash

# Exit immediately if a command exits with a non-zero status
set -e

# Define the addresses of the leader and follower nodes
LEADER_ADDRESS="0.0.0.0:3000"
FOLLOWER_ADDRESS="0.0.0.0:3001"

# Define the external RPC timeout and sync interval
EXTERNAL_RPC_TIMEOUT="2s"
SYNC_INTERVAL="100ms"

# Define colors
COLOR_RESET="\033[0m"
COLOR_TIMESTAMP="\033[1;34m"  # Blue
COLOR_MESSAGE="\033[1;32m"    # Green
COLOR_ADDRESS="\033[1;33m"    # Yellow
COLOR_RESPONSE="\033[1;31m"   # Red

# Function to send a request and get a response
send_request() {
    local url="$1/app=deploy"
    local method=$2
    local params=$3
    response=$(curl -s -S -X POST -H "Content-Type: application/json" --data "{\"jsonrpc\":\"2.0\",\"method\":\"$method\",\"params\":$params,\"id\":1}" $url)
    echo $response
}

# Function to log messages with a timestamp
log() {
    local message=$1
    local address=$2
    local response=$3
    echo -e "${COLOR_TIMESTAMP}$(date '+%Y-%m-%d %H:%M:%S')${COLOR_RESET} - ${COLOR_MESSAGE}$message${COLOR_RESET} - ${COLOR_ADDRESS}Address: $address${COLOR_RESET} - ${COLOR_RESPONSE}Response: $response${COLOR_RESET}"
}

# Validate initial Leader health
leader_health=$(send_request "http://$LEADER_ADDRESS" "stratus_health" "[]")
log "Validating initial Leader health..." "$LEADER_ADDRESS" "$leader_health"
leader_health_result=$(echo $leader_health | jq -r '.result')
if [ "$leader_health_result" != "true" ]; then
    log "Error: Leader node health check failed." "$LEADER_ADDRESS" "$leader_health"
    exit 1
fi

# Validate initial Follower health
# Note: This check will fail if the follower is not sufficiently in sync with the leader.
follower_health=$(send_request "http://$FOLLOWER_ADDRESS" "stratus_health" "[]")
log "Validating initial Follower health..." "$FOLLOWER_ADDRESS" "$follower_health"
follower_health_result=$(echo $follower_health | jq -r '.result')
if [ "$follower_health_result" != "true" ]; then
    log "Error: Follower node health check failed." "$FOLLOWER_ADDRESS" "$follower_health"
    exit 1
fi

# Validate if the Leader is indeed the Leader
leader_state=$(send_request "http://$LEADER_ADDRESS" "stratus_state" "[]")
log "Validating initial Leader state..." "$LEADER_ADDRESS" "$leader_state"
is_leader=$(echo $leader_state | jq -r '.result.is_leader')
if [ "$is_leader" = "true" ]; then
    log "Leader node is correctly identified as the leader." "$LEADER_ADDRESS" "$leader_state"
else
    log "Error: Leader node is not identified as the leader." "$LEADER_ADDRESS" "$leader_state"
    exit 1
fi

# Validate if the Follower is indeed the Follower
follower_state=$(send_request "http://$FOLLOWER_ADDRESS" "stratus_state" "[]")
log "Validating initial Follower state..." "$FOLLOWER_ADDRESS" "$follower_state"
is_leader=$(echo $follower_state | jq -r '.result.is_leader')
if [ "$is_leader" = "false" ]; then
    log "Follower node is correctly identified as the follower." "$FOLLOWER_ADDRESS" "$follower_state"
else
    log "Error: Follower node is not identified as the follower." "$FOLLOWER_ADDRESS" "$follower_state"
    exit 1
fi

# Disable transactions on Leader
disable_leader_tx=$(send_request "http://$LEADER_ADDRESS" "stratus_disableTransactions" "[]")
log "Disabling transactions on Leader..." "$LEADER_ADDRESS" "$disable_leader_tx"
disable_leader_tx_result=$(echo $disable_leader_tx | jq -r '.result')
if [ "$disable_leader_tx_result" != "false" ]; then
    log "Error: Failed to disable transactions on Leader." "$LEADER_ADDRESS" "$disable_leader_tx"
    exit 1
fi

# Disable transactions on Follower
disable_follower_tx=$(send_request "http://$FOLLOWER_ADDRESS" "stratus_disableTransactions" "[]")
log "Disabling transactions on Follower..." "$FOLLOWER_ADDRESS" "$disable_follower_tx"
disable_follower_tx_result=$(echo $disable_follower_tx | jq -r '.result')
if [ "$disable_follower_tx_result" != "false" ]; then
    log "Error: Failed to disable transactions on Follower." "$FOLLOWER_ADDRESS" "$disable_follower_tx"
    exit 1
fi

# Validate Leader state for transaction_enabled
leader_state=$(send_request "http://$LEADER_ADDRESS" "stratus_state" "[]")
log "Validating Leader state for transaction_enabled..." "$LEADER_ADDRESS" "$leader_state"
transaction_enabled=$(echo $leader_state | jq -r '.result.transactions_enabled')
if [ "$transaction_enabled" != "false" ]; then
    log "Error: Leader node transactions are not disabled." "$LEADER_ADDRESS" "$leader_state"
    exit 1
fi

# Validate Follower state for transaction_enabled
follower_state=$(send_request "http://$FOLLOWER_ADDRESS" "stratus_state" "[]")
log "Validating Follower state for transaction_enabled..." "$FOLLOWER_ADDRESS" "$follower_state"
transaction_enabled=$(echo $follower_state | jq -r '.result.transactions_enabled')
if [ "$transaction_enabled" != "false" ]; then
    log "Error: Follower node transactions are not disabled." "$FOLLOWER_ADDRESS" "$follower_state"
    exit 1
fi

# Check for pending transactions on Leader
pending_tx_count=$(send_request "http://$LEADER_ADDRESS" "stratus_pendingTransactionsCount" "[]")
log "Checking pending transactions on Leader..." "$LEADER_ADDRESS" "$pending_tx_count"
pending_tx_count_result=$(echo $pending_tx_count | jq -r '.result')
if [ "$pending_tx_count_result" != "0" ]; then
    log "Error: There are pending transactions on the Leader node." "$LEADER_ADDRESS" "$pending_tx_count"
    exit 1
fi

# Change Leader to Follower
change_to_follower_params="[\"http://$FOLLOWER_ADDRESS/\",\"ws://$FOLLOWER_ADDRESS/\",\"$EXTERNAL_RPC_TIMEOUT\",\"$SYNC_INTERVAL\"]"
change_to_follower=$(send_request "http://$LEADER_ADDRESS" "stratus_changeToFollower" "$change_to_follower_params")
log "Changing Leader to Follower..." "$LEADER_ADDRESS" "$change_to_follower"
change_to_follower_result=$(echo $change_to_follower | jq -r '.result')
if [ "$change_to_follower_result" != "true" ]; then
    log "Error: Failed to change Leader to Follower." "$LEADER_ADDRESS" "$change_to_follower"
    exit 1
fi

# Change Follower to Leader
change_to_leader=$(send_request "http://$FOLLOWER_ADDRESS" "stratus_changeToLeader" "[]")
log "Changing Follower to Leader..." "$FOLLOWER_ADDRESS" "$change_to_leader"
change_to_leader_result=$(echo $change_to_leader | jq -r '.result')
if [ "$change_to_leader_result" != "true" ]; then
    log "Error: Failed to change Follower to Leader." "$FOLLOWER_ADDRESS" "$change_to_leader"
    exit 1
fi

# Enable transactions on new Leader
enable_leader_tx=$(send_request "http://$FOLLOWER_ADDRESS" "stratus_enableTransactions" "[]")
log "Enabling transactions on Leader..." "$FOLLOWER_ADDRESS" "$enable_leader_tx"
enable_leader_tx_result=$(echo $enable_leader_tx | jq -r '.result')
if [ "$enable_leader_tx_result" != "true" ]; then
    log "Error: Failed to enable transactions on Leader." "$FOLLOWER_ADDRESS" "$enable_leader_tx"
    exit 1
fi

# Enable transactions on new Follower
enable_follower_tx=$(send_request "http://$LEADER_ADDRESS" "stratus_enableTransactions" "[]")
log "Enabling transactions on Follower..." "$LEADER_ADDRESS" "$enable_follower_tx"
enable_follower_tx_result=$(echo $enable_follower_tx | jq -r '.result')
if [ "$enable_follower_tx_result" != "true" ]; then
    log "Error: Failed to enable transactions on Follower." "$LEADER_ADDRESS" "$enable_follower_tx"
    exit 1
fi