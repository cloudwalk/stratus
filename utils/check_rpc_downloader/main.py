import os
import sys
import argparse
import psycopg2
from web3 import Web3
from web3.types import HexBytes

def convert_hex_bytes_to_hex_strings(block_dict):
    """Convert HexBytes objects to hex strings in a block dictionary."""
    result = {}
    for key, value in block_dict.items():
        if isinstance(value, (bytes, HexBytes)):
            result[key] = '0x' + value.hex()
        elif isinstance(value, list):
            result[key] = [
                '0x' + item.hex() if isinstance(item, (bytes, HexBytes)) else item 
                for item in value
            ]
        elif isinstance(value, int):
            result[key] = hex(value)
        else:
            result[key] = value
    return result

def get_full_transaction_details(w3, tx_hash):
    """Get transaction details."""
    tx = w3.eth.get_transaction(tx_hash)
    
    # Converter para dict e aplicar a conversão de HexBytes
    tx_dict = convert_hex_bytes_to_hex_strings(tx.__dict__)
    
    # Remover campos internos do Web3
    if '_name' in tx_dict:
        del tx_dict['_name']
    if '_parent' in tx_dict:
        del tx_dict['_parent']
        
    return tx_dict

def normalize_ethereum_address(value):
    """Normalizes Ethereum addresses to lowercase."""
    if isinstance(value, str) and value.startswith('0x') and len(value) == 42:
        return value.lower()
    return value

def normalize_hex_value(value):
    """Normalizes hexadecimal values by removing leading zeros and ensuring consistent format."""
    if isinstance(value, str) and value.startswith('0x'):
        # Remove '0x', strip leading zeros and add '0x' back
        normalized = '0x' + value[2:].lstrip('0')
        # If value was '0x0' or '0x', return '0x0'
        return normalized if normalized != '0x' else '0x0'
    return value

def normalize_block(block):
    """Normalizes block structure to ensure consistency."""
    # List of block fields we want to keep and their order
    block_fields = [
        'hash', 'size', 'miner', 'nonce', 'number', 'uncles',
        'gasUsed', 'mixHash', 'gasLimit', 'extraData', 'logsBloom',
        'stateRoot', 'timestamp', 'difficulty', 'parentHash',
        'sha3Uncles', 'receiptsRoot', 'transactions', 'baseFeePerGas',
        'totalDifficulty', 'transactionsRoot'
    ]
    
    # Transaction fields to normalize
    tx_fields = [
        'r', 's', 'v', 'to', 'gas', 'from', 'hash', 'type',
        'input', 'nonce', 'value', 'chainId', 'gasPrice',
        'blockHash', 'blockNumber', 'transactionIndex'
    ]
    
    normalized = {}
    for field in block_fields:
        if field in block:
            if field == 'transactions':
                normalized[field] = []
                for tx in block[field]:
                    normalized_tx = {}
                    for tx_field in tx_fields:
                        if tx_field in tx:
                            # First normalize addresses
                            value = normalize_ethereum_address(tx[tx_field])
                            # Then normalize hex values
                            value = normalize_hex_value(value)
                            normalized_tx[tx_field] = value
                    normalized[field].append(normalized_tx)
            else:
                # First normalize addresses
                value = normalize_ethereum_address(block[field])
                # Then normalize hex values
                value = normalize_hex_value(value)
                normalized[field] = value
    
    return normalized

def find_differences(dict1, dict2, path=""):
    """Finds and returns differences between two dictionaries."""
    differences = set()  # Using set to avoid duplicates
    for key in set(dict1.keys()) | set(dict2.keys()):
        current_path = f"{path}.{key}" if path else key
        if key not in dict1:
            differences.add(f"Field missing in RPC: {current_path}")
        elif key not in dict2:
            differences.add(f"Field missing in DB: {current_path}")
        elif dict1[key] != dict2[key]:
            if isinstance(dict1[key], dict) and isinstance(dict2[key], dict):
                differences.update(find_differences(dict1[key], dict2[key], current_path))
            elif isinstance(dict1[key], list) and isinstance(dict2[key], list):
                if len(dict1[key]) != len(dict2[key]):
                    differences.add(f"Different lengths in {current_path}: RPC={len(dict1[key])} DB={len(dict2[key])}")
                else:
                    for i, (v1, v2) in enumerate(zip(dict1[key], dict2[key])):
                        if isinstance(v1, dict) and isinstance(v2, dict):
                            differences.update(find_differences(v1, v2, f"{current_path}[{i}]"))
                        elif v1 != v2:
                            differences.add(f"Different value in {current_path}[{i}]: RPC={v1} DB={v2}")
            else:
                differences.add(f"Different value in {current_path}: RPC={dict1[key]} DB={dict2[key]}")
    return sorted(differences)  # Returns a sorted list of differences

def check_blocks(w3, conn, start_block, end_block):
    """
    For each block number in the specified range,
    - Fetches the block via RPC (eth_getBlockByNumber)
    - Retrieves the corresponding record from the external_blocks table
    - Compares the block obtained from the blockchain with the stored block.
    """
    cursor = conn.cursor()
    all_successful = True

    for block_num in range(start_block, end_block + 1):
        try:
            block_rpc = w3.eth.get_block(block_num)
            rpc_block = block_rpc.__dict__.copy()
            
            # Buscar detalhes completos para cada transação
            full_transactions = []
            for tx_hash in block_rpc.transactions:
                tx_details = get_full_transaction_details(w3, tx_hash)
                full_transactions.append(tx_details)
            
            rpc_block['transactions'] = full_transactions
            rpc_block = convert_hex_bytes_to_hex_strings(rpc_block)
            
            # Remove campos internos do Web3
            if '_name' in rpc_block:
                del rpc_block['_name']
            if '_parent' in rpc_block:
                del rpc_block['_parent']

            cursor.execute("SELECT block FROM external_blocks WHERE number = %s;", (block_num,))
            result = cursor.fetchone()

            if result is None:
                print(f"[WARNING] Block {block_num} not found in the external_blocks table.")
                all_successful = False
            else:
                db_block = result[0]
                
                # Normalizar ambos os blocos antes de comparar
                normalized_rpc = normalize_block(rpc_block)
                normalized_db = normalize_block(db_block)
                
                if normalized_rpc != normalized_db:
                    print(f"[ERROR] Block {block_num}: content mismatch")
                    differences = find_differences(normalized_rpc, normalized_db)
                    print("\nDiferenças encontradas:")
                    for diff in differences:
                        print(f"  - {diff}")
                    all_successful = False
                else:
                    print(f"[OK] Block {block_num} verified.")

        except Exception as e:
            print(f"[ERROR] Failed to process block {block_num}: {e}")
            all_successful = False
            continue

    cursor.close()
    return all_successful

def main():
    parser = argparse.ArgumentParser(
        description="Verifies if the external_blocks table contains all blocks and if the blocks match the Ethereum blockchain."
    )
    parser.add_argument("--start", type=int, required=True, help="Starting block number to verify.")
    args = parser.parse_args()

    # Postgres settings (can be defined via environment variables or default values)
    pg_host     = os.getenv("POSTGRES_HOST", "localhost")
    pg_port     = os.getenv("POSTGRES_PORT", "5432")
    pg_user     = os.getenv("POSTGRES_USER", "postgres")
    pg_password = os.getenv("POSTGRES_PASSWORD", "password")
    pg_db       = os.getenv("POSTGRES_DB", "my_database")
    
    # Ethereum RPC configuration
    eth_rpc_url = os.getenv("ETH_RPC_URL", "http://localhost:8545")

    # Connect to Postgres database
    try:
        conn = psycopg2.connect(
            host=pg_host,
            port=pg_port,
            user=pg_user,
            password=pg_password,
            dbname=pg_db
        )
    except Exception as e:
        print(f"[ERROR] Failed to connect to Postgres: {e}")
        sys.exit(1)

    # Connect to Ethereum node via Web3
    w3 = Web3(Web3.HTTPProvider(eth_rpc_url))
    if not w3.is_connected():
        print("Failed to connect to Ethereum node")
        sys.exit(1)

    # Get the latest block directly from the blockchain
    try:
        end_block = w3.eth.block_number
        print(f"Latest block on blockchain: {end_block}")
    except Exception as e:
        print(f"[ERROR] Failed to obtain the latest block from the blockchain: {e}")
        conn.close()
        sys.exit(1)

    print(f"Starting verification of blocks from {args.start} to {end_block}...")
    if check_blocks(w3, conn, args.start, end_block):
        print("\nSUCCESS: All blocks were found and all blocks match!")
        conn.close()
        sys.exit(0)
    else:
        print("\nWARNING: Discrepancies or missing blocks were found!")
        conn.close()
        sys.exit(1)

if __name__ == "__main__":
    main()