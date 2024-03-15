SELECT
    address as "address: _",
    latest_nonce as "nonce: _",
    latest_balance as "balance: _",
    bytecode as "bytecode: _"
FROM accounts
WHERE address = $1
