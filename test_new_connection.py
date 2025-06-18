#!/usr/bin/env python3
"""
Test sending transactions using a new HTTP connection for each transaction.
This might help avoid connection state issues in Stratus parallel mode.
"""

import time
import sys
import json
from web3 import Web3
from eth_account import Account

def send_transaction_with_new_connection(rpc_url, account, nonce, value):
    """Send a transaction using a fresh connection"""
    # Create new connection for this transaction
    w3 = Web3(Web3.HTTPProvider(rpc_url, request_kwargs={'timeout': 30}))

    if not w3.is_connected():
        raise Exception("Failed to connect to Stratus")

    # Create and sign transaction
    tx = {
        'nonce': nonce,
        'to': account.address,
        'value': value,
        'gas': 21000,
        'gasPrice': 0,
        'chainId': w3.eth.chain_id
    }

    signed_tx = account.sign_transaction(tx)

    # Send transaction
    tx_hash = w3.eth.send_raw_transaction(signed_tx.raw_transaction)

    return tx_hash.hex(), signed_tx.hash.hex()

def main():
    # Configuration
    RPC_URL = "http://localhost:3000"
    PRIVATE_KEY = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
    NUM_TRANSACTIONS = 10

    print("=== Stratus Parallel Mode - New Connection Per Transaction Test ===\n")

    # Setup account
    account = Account.from_key(PRIVATE_KEY)
    print(f"👤 Account: {account.address}")

    # Get initial state using a temporary connection
    w3_temp = Web3(Web3.HTTPProvider(RPC_URL))
    if not w3_temp.is_connected():
        print("❌ Cannot connect to Stratus")
        sys.exit(1)

    base_nonce = w3_temp.eth.get_transaction_count(account.address)
    base_block = w3_temp.eth.block_number

    print(f"📊 Initial state:")
    print(f"   Starting nonce: {base_nonce}")
    print(f"   Starting block: {base_block}")
    print(f"   Will send {NUM_TRANSACTIONS} transactions\n")

    # Send transactions
    results = []
    successful = 0
    failed = 0

    print("🚀 Sending transactions (new connection for each)...")
    start_time = time.time()

    for i in range(NUM_TRANSACTIONS):
        nonce = base_nonce + i
        value = i  # Different value for each transaction

        print(f"\n📤 Transaction {i+1}/{NUM_TRANSACTIONS}:")
        print(f"   Nonce: {nonce}, Value: {value} wei")

        try:
            # Send transaction with new connection
            tx_start = time.time()
            tx_hash, expected_hash = send_transaction_with_new_connection(
                RPC_URL, account, nonce, value
            )
            tx_time = time.time() - tx_start

            print(f"   ✅ Sent successfully in {tx_time:.2f}s")
            print(f"   Hash: {tx_hash}")

            successful += 1
            results.append({
                'index': i,
                'nonce': nonce,
                'success': True,
                'hash': tx_hash,
                'time': tx_time
            })

            # Small delay between transactions
            time.sleep(0.5)

        except Exception as e:
            print(f"   ❌ Failed: {e}")
            failed += 1
            results.append({
                'index': i,
                'nonce': nonce,
                'success': False,
                'error': str(e)
            })

            # Longer delay after failure
            time.sleep(2)

    total_time = time.time() - start_time

    # Final check
    print("\n📊 Final results:")
    print(f"   Total time: {total_time:.2f}s")
    print(f"   Successful: {successful}/{NUM_TRANSACTIONS}")
    print(f"   Failed: {failed}/{NUM_TRANSACTIONS}")

    if successful > 0:
        print(f"   Average time per tx: {total_time/successful:.2f}s")
        print(f"   Throughput: {successful/total_time:.2f} tx/s")

    # Check final state
    try:
        w3_final = Web3(Web3.HTTPProvider(RPC_URL))
        final_nonce = w3_final.eth.get_transaction_count(account.address)
        final_block = w3_final.eth.block_number

        print(f"\n📈 State changes:")
        print(f"   Nonce: {base_nonce} → {final_nonce} (expected {base_nonce + successful})")
        print(f"   Block: {base_block} → {final_block}")

        # Check if transactions were actually processed
        if final_nonce > base_nonce:
            print(f"\n✅ Transactions were processed! Nonce increased by {final_nonce - base_nonce}")
        else:
            print(f"\n⚠️  No nonce increase detected. Transactions may not have been processed.")

    except Exception as e:
        print(f"\n❌ Error checking final state: {e}")

    # Save results
    with open('new_connection_test_results.json', 'w') as f:
        json.dump({
            'configuration': {
                'rpc_url': RPC_URL,
                'num_transactions': NUM_TRANSACTIONS
            },
            'results': results,
            'summary': {
                'successful': successful,
                'failed': failed,
                'total_time': total_time,
                'base_nonce': base_nonce,
                'base_block': base_block
            }
        }, f, indent=2)

    print(f"\n💾 Results saved to new_connection_test_results.json")

    return successful > 0

if __name__ == "__main__":
    print("This test creates a new HTTP connection for each transaction.")
    print("This might help avoid connection state issues in parallel mode.\n")
    print("Make sure Stratus is running in parallel mode:")
    print("stratus --leader --executor-strategy parallel --executor-evms 5 --block-mode 1s\n")

    response = input("Is Stratus running in parallel mode? (y/n): ")
    if response.lower() != 'y':
        print("Please start Stratus first.")
        sys.exit(1)

    print()
    success = main()

    if success:
        print("\n🎉 SUCCESS: New connection approach works!")
        print("This confirms the issue might be related to connection state.")
    else:
        print("\n❌ FAILED: Even with new connections, transactions fail.")
        print("The issue is deeper than connection management.")
