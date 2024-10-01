import argparse
import requests
import time
from dataclasses import dataclass
from datetime import datetime
from enum import Enum
from typing import Any, Dict, Optional

# Define colors
class Color:
    RESET = "\033[0m"
    TIMESTAMP = "\033[1;30m" # Gray
    MESSAGE = "\033[1;37m"   # White
    ADDRESS = "\033[1;34m"   # Light Blue
    RESPONSE = "\033[1;32m"  # Light Green
    ERROR = "\033[1;31m"     # Red

class NodeRole(Enum):
    LEADER = "Leader"
    FOLLOWER = "Follower"

@dataclass
class Config:
    leader_address: str
    follower_address: str
    auto_approve: bool
    external_rpc_timeout: str = "2s"
    sync_interval: str = "100ms"
    total_check_attempts: int = 10
    required_consecutive_checks: int = 3
    sleep_interval: float = 0.5

# Custom exception
class DeploymentError(Exception):
    def __init__(self, message: str, error_type: Optional[str] = None):
        super().__init__(message)
        self.error_type = error_type

# Function to send a request and get a response
def send_request(url: str, method: str, params: list) -> Dict[str, Any]:
    try:
        response = requests.post(
            url, json={"jsonrpc": "2.0", "method": method, "params": params, "id": 1}, timeout=30
        )
        return response.json()
    except requests.exceptions.ConnectionError as e:
        log(message=f"Error: Unable to connect to {url}. Please check the network connection and the node status.", address=url, response={"error": str(e)}, error=True)
        raise DeploymentError(f"Unable to connect to {url}", error_type="ConnectionError")

# Function to log messages with a timestamp
def log(message: str, address: Optional[str] = None, response: Optional[Dict[str, Any]] = None, error: bool = False) -> None:
    color = Color.ERROR if error else Color.MESSAGE
    log_message = f"{Color.TIMESTAMP}{datetime.now().strftime('%Y-%m-%d %H:%M:%S')}{Color.RESET} - {color}{message}{Color.RESET}"
    if address:
        log_message += f" - {Color.ADDRESS}Address: {address}{Color.RESET}"
    if response:
        log_message += f" - {Color.RESPONSE}Response: {response}{Color.RESET}"
    print(log_message)

# Function to validate node health
def validate_health(address: str, role: NodeRole) -> None:
    health = send_request(url=f"http://{address}", method="stratus_health", params=[])
    log(message=f"Validating {role.name} health...", address=address)
    if health.get("result") != True:
        log(message=f"Error: {role.name} node health check failed.", address=address, response=health)
        raise DeploymentError(f"{role.name} node health check failed", error_type="HealthCheckError")
    else:
        log(message=f"{role.name} node health check passed.", address=address, response=health)

# Function to validate node state
def validate_state(address: str, expected_state: bool, role: NodeRole) -> None:
    state = send_request(url=f"http://{address}", method="stratus_state", params=[])
    log(message=f"Validating {role.name} state...", address=address)
    if state.get("result", {}).get("is_leader") != expected_state:
        log(message=f"Error: {role.name} node is not identified as expected.", address=address, response=state)
        raise DeploymentError(f"{role.name} node is not identified as expected", error_type="StateCheckError")
    else:
        log(message=f"{role.name} node is correctly identified as expected.", address=address, response=state)

# Function to toggle transactions
def toggle_transactions(address: str, enable: bool, role: NodeRole) -> None:
    method = "stratus_enableTransactions" if enable else "stratus_disableTransactions"
    action = "Enabling" if enable else "Disabling"
    result_check = enable

    response = send_request(url=f"http://{address}", method=method, params=[])
    log(message=f"{action} transactions on {role.name}...", address=address)
    if response.get("result") != result_check:
        log(message=f"Error: Failed to {action.lower()} transactions on {role.name}.", address=address, response=response)
        raise DeploymentError(f"Failed to {action.lower()} transactions on {role.name}", error_type="TransactionToggleError")
    else:
        log(message=f"Successfully {action.lower()} transactions on {role.name}.", address=address, response=response)

# Function to toggle miner
def toggle_miner(address: str, enable: bool) -> None:
    method = "stratus_enableMiner" if enable else "stratus_disableMiner"
    action = "Enabling" if enable else "Disabling"
    result_check = enable

    response = send_request(url=f"http://{address}", method=method, params=[])
    log(message=f"{action} miner on node...", address=address)
    if response.get("result") != result_check:
        log(message=f"Error: Failed to {action.lower()} miner on node.", address=address, response=response)
        raise DeploymentError(f"Failed to {action.lower()} miner on node", error_type="MinerToggleError")
    else:
        log(message=f"Successfully {action.lower()} miner on node.", address=address, response=response)

# Function to check pending transactions
def has_pending_transactions(address: str, total_attempts: int, required_checks: int, sleep_interval: float) -> bool:
    success_count = 0
    for i in range(1, total_attempts + 1):
        pending_tx_count = send_request(url=f"http://{address}", method="stratus_pendingTransactionsCount", params=[])
        log(message=f"Checking pending transactions on LEADER (Attempt {i})...", address=address)
        if pending_tx_count.get("result") == 0:
            success_count += 1
            log(message=f"No pending transactions on the LEADER node (Attempt {i}).", address=address, response=pending_tx_count)
        else:
            success_count = 0
            log(message=f"Error: There are pending transactions on the LEADER node (Attempt {i}).", address=address, response=pending_tx_count)
        if success_count == required_checks:
            return True
        time.sleep(sleep_interval)
    log(message=f"Error: Failed to confirm no pending transactions on the LEADER node after {total_attempts} attempts.", address=address)
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
            log(message=f"FOLLOWER is in sync with the LEADER (Attempt {i}).", address=follower_address, response=follower_block_number)
        else:
            import_success_count = 0
            previous_block_number = leader_block_number_result
            log(message=f"Error: FOLLOWER is not in sync with the LEADER (Attempt {i}).", address=follower_address, response=follower_block_number)
        if import_success_count == required_checks:
            return True
        time.sleep(sleep_interval)
    log(message=f"Error: Failed to confirm block import on FOLLOWER or new blocks are being generated after {total_attempts} attempts.", address=follower_address)
    return False

# Function to change node roles
def change_role(address: str, method: str, params: list, role: NodeRole) -> None:
    change_role = send_request(url=f"http://{address}", method=method, params=params)
    log(message=f"Changing to {role.name}...", address=address)
    if change_role.get("result") != True:
        log(message=f"Error: Failed to change to {role.name}.", address=address, response=change_role)
        raise DeploymentError(f"Failed to change to {role.name}", error_type="RoleChangeError")
    else:
        log(message=f"Successfully changed to {role.name}.", address=address, response=change_role)

# Function to check if leader or follower are leaders
def has_leader(leader_address: str, follower_address: str) -> bool:
    nodes_to_check = [leader_address, follower_address]
    for node in nodes_to_check:
        state = send_request(url=f"http://{node}", method="stratus_state", params=[])
        log(message=f"Checking if node is a LEADER...", address=node)
        if state.get("result", {}).get("is_leader"):
            log(message=f"Error: Node is currently a LEADER.", address=node, response=state)
            raise DeploymentError(f"Node {node} is currently a LEADER", error_type="LeaderCheckError")
        else:
            log(message=f"Node is not a LEADER.", address=node, response=state)
    return False

def main() -> None:
    parser = argparse.ArgumentParser(description="Deploy script")
    parser.add_argument("--current-leader", required=True, help="Current Leader address")
    parser.add_argument("--current-follower", required=True, help="Current Follower address")
    parser.add_argument("--auto-approve", action="store_true", help="Auto approve actions")
    args = parser.parse_args()

    try:
        config = Config(
            leader_address=args.current_leader,
            follower_address=args.current_follower,
            auto_approve=args.auto_approve
        )
    except AttributeError as e:
        log(message=f"Error initializing Config: {str(e)}", error=True)
        raise DeploymentError("Error initializing Config", error_type="InitializationError")

    if config.leader_address == config.follower_address:
        log(message="Error: Leader and follower addresses must be different.")
        raise DeploymentError("Leader and follower addresses must be different", error_type="InputValidationError")

    start_time = time.time()

    try:
        # Validate initial health and state
        validate_health(address=config.leader_address, role=NodeRole.LEADER)
        validate_health(address=config.follower_address, role=NodeRole.FOLLOWER)
        validate_state(address=config.leader_address, expected_state=True, role=NodeRole.LEADER)
        validate_state(address=config.follower_address, expected_state=False, role=NodeRole.FOLLOWER)

        if not config.auto_approve:
            confirmation = input("Do you want to proceed with pausing transactions? (yes/no): ")
            if confirmation.lower() != "yes":
                print("Script canceled.")
                return

        # Disable transactions
        toggle_transactions(address=config.leader_address, enable=False, role=NodeRole.LEADER)
        toggle_transactions(address=config.follower_address, enable=False, role=NodeRole.FOLLOWER)

        # Check pending transactions
        if not has_pending_transactions(address=config.leader_address, total_attempts=config.total_check_attempts, required_checks=config.required_consecutive_checks, sleep_interval=config.sleep_interval):
            toggle_transactions(address=config.leader_address, enable=True, role=NodeRole.LEADER)
            toggle_transactions(address=config.follower_address, enable=True, role=NodeRole.FOLLOWER)
            return

        if not config.auto_approve:
            confirmation = input("Do you want to proceed with pausing the miner? (yes/no): ")
            if confirmation.lower() != "yes":
                print("Script canceled.")
                # Toggle transactions back on
                toggle_transactions(address=config.leader_address, enable=True, role=NodeRole.LEADER)
                toggle_transactions(address=config.follower_address, enable=True, role=NodeRole.FOLLOWER)
                return

        # Pause miner on Leader
        toggle_miner(address=config.leader_address, enable=False)

        # Check block synchronization
        if not blocks_synced(leader_address=config.leader_address, follower_address=config.follower_address, total_attempts=config.total_check_attempts, required_checks=config.required_consecutive_checks, sleep_interval=config.sleep_interval):
            toggle_miner(address=config.leader_address, enable=True)
            toggle_transactions(address=config.leader_address, enable=True, role=NodeRole.LEADER)
            toggle_transactions(address=config.follower_address, enable=True, role=NodeRole.FOLLOWER)
            return

        if not config.auto_approve:
            confirmation = input("Do you want to proceed with changing the Leader to Follower? (yes/no): ")
            if confirmation.lower() != "yes":
                print("Script canceled.")
                # Toggle miner back on
                toggle_miner(address=config.leader_address, enable=True)
                # Toggle transactions back on
                toggle_transactions(address=config.leader_address, enable=True, role=NodeRole.LEADER)
                toggle_transactions(address=config.follower_address, enable=True, role=NodeRole.FOLLOWER)
                return

        # Change Leader to Follower
        change_to_follower_params = [f"http://{config.follower_address}/", f"ws://{config.follower_address}/", config.external_rpc_timeout, config.sync_interval]
        change_role(address=config.leader_address, method="stratus_changeToFollower", params=change_to_follower_params, role=NodeRole.FOLLOWER)

        # Validate new Follower state
        validate_state(address=config.leader_address, expected_state=False, role=NodeRole.FOLLOWER)

        # Check if any leader or follower are leaders
        if has_leader(config.leader_address, config.follower_address):
            log(message="Error: One of the nodes is currently a leader. Aborting leader switch.")
            return

        if not config.auto_approve:
            confirmation = input("Do you want to proceed changing the Follower to Leader? (yes/no): ")
            if confirmation.lower() != "yes":
                print("Script canceled.")
                return

        # Change Follower to Leader
        change_role(address=config.follower_address, method="stratus_changeToLeader", params=[], role=NodeRole.LEADER)

        # Validate new Leader state
        validate_state(address=config.follower_address, expected_state=True, role=NodeRole.LEADER)

        # Enable transactions on new Leader and Follower
        toggle_transactions(address=config.follower_address, enable=True, role=NodeRole.LEADER)
        toggle_transactions(address=config.leader_address, enable=True, role=NodeRole.FOLLOWER)

        end_time = time.time()
        elapsed_time = end_time - start_time
        log(message=f"Process completed in {elapsed_time:.2f} seconds.")
    except DeploymentError as e:
        log(message=f"Process failed: {str(e)}", error=True)

if __name__ == "__main__":
    main()
