import argparse
import requests
import time
from datetime import datetime
from typing import Any, Dict, List, Optional

# Define colors
COLOR_RESET = "\033[0m"
COLOR_TIMESTAMP = "\033[1;30m"  # Gray
COLOR_MESSAGE = "\033[1;37m"  # White
COLOR_ADDRESS = "\033[1;34m"  # Light Blue
COLOR_RESPONSE = "\033[1;32m"  # Light Green
COLOR_ERROR = "\033[1;31m"  # Red

# Function to send a request and get a response
def send_request(url: str, method: str, params: list) -> Dict[str, Any]:
    try:
        response = requests.post(
            url, json={"jsonrpc": "2.0", "method": method, "params": params, "id": 1}, timeout=30
        )
        return response.json()
    except requests.exceptions.ConnectionError as e:
        log(message=f"Error: Unable to connect to {url}. Please check the network connection and the node status.", address=url, response={"error": str(e)}, error=True)
        exit(1)

# Function to log messages with a timestamp
def log(message: str, address: Optional[str] = None, response: Optional[Dict[str, Any]] = None, error: bool = False) -> None:
    color = COLOR_ERROR if error else COLOR_MESSAGE
    log_message = f"{COLOR_TIMESTAMP}{datetime.now().strftime('%Y-%m-%d %H:%M:%S')}{COLOR_RESET} - {color}{message}{COLOR_RESET}"
    if address:
        log_message += f" - {COLOR_ADDRESS}Address: {address}{COLOR_RESET}"
    if response:
        log_message += f" - {COLOR_RESPONSE}Response: {response}{COLOR_RESET}"
    print(log_message)

# Function to validate node health
def validate_health(address: str, role: str) -> None:
    health = send_request(url=f"http://{address}", method="stratus_health", params=[])
    log(message=f"Validating {role} health...", address=address)
    if health.get("result") != True:
        log(message=f"Error: {role} node health check failed.", address=address, response=health)
        exit(1)
    else:
        log(message=f"{role} node health check passed.", address=address, response=health)

# Function to validate node state
def validate_state(address: str, expected_state: bool, role: str) -> None:
    state = send_request(url=f"http://{address}", method="stratus_state", params=[])
    log(message=f"Validating {role} state...", address=address)
    if state.get("result", {}).get("is_leader") != expected_state:
        log(message=f"Error: {role} node is not identified as expected.", address=address, response=state)
        exit(1)
    else:
        log(message=f"{role} node is correctly identified as expected.", address=address, response=state)

# Function to toggle transactions
def toggle_transactions(address: str, enable: bool, role: str) -> None:
    method = "stratus_enableTransactions" if enable else "stratus_disableTransactions"
    action = "Enabling" if enable else "Disabling"
    result_check = enable

    response = send_request(url=f"http://{address}", method=method, params=[])
    log(message=f"{action} transactions on {role}...", address=address)
    if response.get("result") != result_check:
        log(message=f"Error: Failed to {action.lower()} transactions on {role}.", address=address, response=response)
        exit(1)
    else:
        log(message=f"Successfully {action.lower()} transactions on {role}.", address=address, response=response)

# Function to toggle miner
def toggle_miner(address: str, enable: bool) -> None:
    method = "stratus_enableMiner" if enable else "stratus_disableMiner"
    action = "Enabling" if enable else "Disabling"
    result_check = enable

    response = send_request(url=f"http://{address}", method=method, params=[])
    log(message=f"{action} miner on node...", address=address)
    if response.get("result") != result_check:
        log(message=f"Error: Failed to {action.lower()} miner on node.", address=address, response=response)
        exit(1)
    else:
        log(message=f"Successfully {action.lower()} miner on node.", address=address, response=response)

# Function to check pending transactions
def has_pending_transactions(address: str, total_attempts: int, required_checks: int, sleep_interval: float) -> bool:
    success_count = 0
    for i in range(1, total_attempts + 1):
        pending_tx_count = send_request(url=f"http://{address}", method="stratus_pendingTransactionsCount", params=[])
        log(message=f"Checking pending transactions on Leader (Attempt {i})...", address=address)
        if pending_tx_count.get("result") == 0:
            success_count += 1
            log(message=f"No pending transactions on the Leader node (Attempt {i}).", address=address, response=pending_tx_count)
        else:
            success_count = 0
            log(message=f"Error: There are pending transactions on the Leader node (Attempt {i}).", address=address, response=pending_tx_count)
        if success_count == required_checks:
            return True
        time.sleep(sleep_interval)
    log(message=f"Error: Failed to confirm no pending transactions on the Leader node after {total_attempts} attempts.", address=address)
    return False

# Function to check block synchronization
def blocks_synced(leader_address: str, follower_address: str, total_attempts: int, required_checks: int, sleep_interval: float) -> bool:
    import_success_count = 0
    previous_block_number = ""
    for i in range(1, total_attempts + 1):
        leader_block_number = send_request(url=f"http://{leader_address}", method="eth_blockNumber", params=[])
        follower_block_number = send_request(url=f"http://{follower_address}", method="eth_blockNumber", params=[])
        log(message=f"Checking block numbers (Attempt {i})...", address=leader_address, response=follower_block_number)
        leader_block_number_result = leader_block_number.get("result")
        follower_block_number_result = follower_block_number.get("result")
        if not previous_block_number:
            previous_block_number = leader_block_number_result
            log(message=f"First check, setting previous block number (Attempt {i}).", address=leader_address, response=follower_block_number)
        elif leader_block_number_result == follower_block_number_result and leader_block_number_result == previous_block_number:
            import_success_count += 1
            log(message=f"Follower is in sync with the Leader (Attempt {i}).", address=follower_address, response=follower_block_number)
        else:
            import_success_count = 0
            previous_block_number = leader_block_number_result
            log(message=f"Error: Follower is not in sync with the Leader (Attempt {i}).", address=follower_address, response=follower_block_number)
        if import_success_count == required_checks:
            return True
        time.sleep(sleep_interval)
    log(message=f"Error: Failed to confirm block import on Follower or new blocks are being generated after {total_attempts} attempts.", address=follower_address)
    return False

# Function to change node roles
def change_role(address: str, method: str, params: list, role: str) -> None:
    change_role = send_request(url=f"http://{address}", method=method, params=params)
    log(message=f"Changing to {role}...", address=address)
    if change_role.get("result") != True:
        log(message=f"Error: Failed to change to {role}.", address=address, response=change_role)
        exit(1)
    else:
        log(message=f"Successfully changed to {role}.", address=address, response=change_role)

# Function to check if any extra nodes, leader, or follower are leaders
def has_leader(leader_address: str, follower_address: str, extra_nodes: List[str]) -> bool:
    nodes_to_check = [leader_address, follower_address] + extra_nodes
    for node in nodes_to_check:
        state = send_request(url=f"http://{node}", method="stratus_state", params=[])
        log(message=f"Checking if node is a leader...", address=node)
        if state.get("result", {}).get("is_leader"):
            log(message=f"Error: Node is currently a leader.", address=node, response=state)
            return True
        else:
            log(message=f"Node is not a leader.", address=node, response=state)
    return False

def main() -> None:
    parser = argparse.ArgumentParser(description="Deploy script")
    parser.add_argument("--leader", required=True, help="Leader address")
    parser.add_argument("--follower", required=True, help="Follower address")
    parser.add_argument("--extra-nodes", nargs='*', help="List of extra nodes")
    parser.add_argument("--auto-approve", action="store_true", help="Auto approve actions")
    args = parser.parse_args()

    LEADER_ADDRESS = args.leader
    FOLLOWER_ADDRESS = args.follower
    EXTRA_NODES = args.extra_nodes or []
    AUTO_APPROVE = args.auto_approve

    if LEADER_ADDRESS == FOLLOWER_ADDRESS:
        log(message="Error: Leader and follower addresses must be different.")
        exit(1)

    if LEADER_ADDRESS in EXTRA_NODES or FOLLOWER_ADDRESS in EXTRA_NODES:
        log(message="Error: Extra nodes list must not contain the leader or follower addresses.")
        exit(1)

    EXTERNAL_RPC_TIMEOUT = "2s"
    SYNC_INTERVAL = "100ms"
    TOTAL_CHECK_ATTEMPTS = 10
    REQUIRED_CONSECUTIVE_CHECKS = 3
    SLEEP_INTERVAL = 0.5

    start_time = time.time()

    # Validate initial health and state
    validate_health(address=LEADER_ADDRESS, role="Leader")
    validate_health(address=FOLLOWER_ADDRESS, role="Follower")
    validate_state(address=LEADER_ADDRESS, expected_state=True, role="Leader")
    validate_state(address=FOLLOWER_ADDRESS, expected_state=False, role="Follower")

    if not AUTO_APPROVE:
        confirmation = input("Do you want to proceed with pausing transactions? (yes/no): ")
        if confirmation.lower() != "yes":
            print("Script canceled.")
            exit(1)

    # Disable transactions
    toggle_transactions(address=LEADER_ADDRESS, enable=False, role="Leader")
    toggle_transactions(address=FOLLOWER_ADDRESS, enable=False, role="Follower")

    # Check pending transactions
    if not has_pending_transactions(address=LEADER_ADDRESS, total_attempts=TOTAL_CHECK_ATTEMPTS, required_checks=REQUIRED_CONSECUTIVE_CHECKS, sleep_interval=SLEEP_INTERVAL):
        toggle_transactions(address=LEADER_ADDRESS, enable=True, role="Leader")
        toggle_transactions(address=FOLLOWER_ADDRESS, enable=True, role="Follower")
        exit(1)
    
    if not AUTO_APPROVE:
        confirmation = input("Do you want to proceed with pausing the miner? (yes/no): ")
        if confirmation.lower() != "yes":
            print("Script canceled.")
            # Toggle transactions back on
            toggle_transactions(address=LEADER_ADDRESS, enable=True, role="Leader")
            toggle_transactions(address=FOLLOWER_ADDRESS, enable=True, role="Follower")
            exit(1)

    # Pause miner on Leader
    toggle_miner(address=LEADER_ADDRESS, enable=False)

    # Check block synchronization
    if not blocks_synced(leader_address=LEADER_ADDRESS, follower_address=FOLLOWER_ADDRESS, total_attempts=TOTAL_CHECK_ATTEMPTS, required_checks=REQUIRED_CONSECUTIVE_CHECKS, sleep_interval=SLEEP_INTERVAL):
        toggle_miner(address=LEADER_ADDRESS, enable=True)
        toggle_transactions(address=LEADER_ADDRESS, enable=True, role="Leader")
        toggle_transactions(address=FOLLOWER_ADDRESS, enable=True, role="Follower")
        exit(1)

    if not AUTO_APPROVE:
        confirmation = input("Do you want to proceed with changing the Leader to Follower? (yes/no): ")
        if confirmation.lower() != "yes":
            print("Script canceled.")
            # Toggle miner back on
            toggle_miner(address=LEADER_ADDRESS, enable=True)
            # Toggle transactions back on
            toggle_transactions(address=LEADER_ADDRESS, enable=True, role="Leader")
            toggle_transactions(address=FOLLOWER_ADDRESS, enable=True, role="Follower")
            exit(1)
            
    # Change Leader to Follower
    change_to_follower_params = [f"http://{FOLLOWER_ADDRESS}/", f"ws://{FOLLOWER_ADDRESS}/", EXTERNAL_RPC_TIMEOUT, SYNC_INTERVAL]
    change_role(address=LEADER_ADDRESS, method="stratus_changeToFollower", params=change_to_follower_params, role="Follower")

    # Validate new Follower state
    validate_state(address=LEADER_ADDRESS, expected_state=False, role="Follower")

    # Check if any extra nodes, leader, or follower are leaders
    if has_leader(LEADER_ADDRESS, FOLLOWER_ADDRESS, EXTRA_NODES):
        log(message="Error: One of the nodes is currently a leader. Aborting leader switch.")
        exit(1)

    # Change Follower to Leader
    change_role(address=FOLLOWER_ADDRESS, method="stratus_changeToLeader", params=[], role="Leader")

    # Validate new Leader state
    validate_state(address=FOLLOWER_ADDRESS, expected_state=True, role="Leader")

    # Enable transactions on new Leader and Follower
    toggle_transactions(address=FOLLOWER_ADDRESS, enable=True, role="Leader")
    toggle_transactions(address=LEADER_ADDRESS, enable=True, role="Follower")

    end_time = time.time()
    elapsed_time = end_time - start_time
    log(message=f"Process completed in {elapsed_time:.2f} seconds.")

if __name__ == "__main__":
    main()