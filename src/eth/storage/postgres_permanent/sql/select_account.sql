
SELECT
    address as "address: _",
    latest_nonce as "nonce: _",
    latest_balance as "balance: _",
    bytecode as "bytecode: _",
    code_hash as "code_hash: _",
    slot_indexes_static_access as "slot_indexes_static_access: _",
    slot_indexes_static_access as "slot_indexes_mapping_access: _"
FROM accounts
WHERE address = $1
