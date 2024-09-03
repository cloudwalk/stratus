from web3 import Web3

# Connect to an Ethereum node (replace with your own endpoint)
w3 = Web3(Web3.HTTPProvider("https://mainnet.infura.io/v3/YOUR-PROJECT-ID"))

transaction = {
    "nonce": 10,
    "to": "0x51fda8b52cfad8abedbdc4095b04828ea22673d9",
    "value": w3.to_wei(0.1, "ether"),
    "gas": 21000,
    "gasPrice": w3.eth.gas_price,
    "chainId": 1,  # 1 for Ethereum mainnet
}

# Sign the transaction
signed_txn = w3.eth.account.sign_transaction(
    transaction, "0x51fda8b52cfad8abedbdc4095b04828ea22673d9"
)

# Get the transaction hash
tx_hash = w3.to_hex(signed_txn.hash)
