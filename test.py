from web3 import Web3

w3 = Web3(Web3.HTTPProvider('http://127.0.0.1:3000'))

alice = w3.eth.account.create()
bob = w3.eth.account.create()

transaction = {
    "from": alice.address,
    "to": bob.address,
    "value": 0,
    "gasPrice": 0,
    "gas": 10000000,
    "chainId": 2008,
    "nonce": 0
    }

signed = w3.eth.account.sign_transaction(transaction, alice.key)
print(signed.rawTransaction.hex());
