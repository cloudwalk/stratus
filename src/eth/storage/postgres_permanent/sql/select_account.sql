
SELECT
    address as "address: _",
    latest_nonce as "nonce: _",
    latest_balance as "balance: _",
    bytecode as "bytecode: _",
    code_hash as "code_hash: _"
FROM accounts
WHERE address = $1
