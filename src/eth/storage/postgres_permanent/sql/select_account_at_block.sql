WITH closest_nonce_block AS (
    SELECT MAX(block_number) as block_number
    FROM historical_nonces
    WHERE block_number <= $2
    AND address = $1
),
closest_balance_block AS (
    SELECT MAX(block_number) as block_number
    FROM historical_balances
    WHERE block_number <= $2
    AND address = $1
),
closest_nonce AS (
    SELECT nonce
    FROM historical_nonces
    JOIN closest_nonce_block ON true
    WHERE historical_nonces.block_number = closest_nonce_block.block_number
    AND address = $1
),
closest_balance AS (
    SELECT balance
    FROM historical_balances
    JOIN closest_balance_block ON true
    WHERE historical_balances.block_number = closest_balance_block.block_number
    AND address = $1
)
SELECT
    accounts.address as "address: _",
    closest_nonce.nonce as "nonce: _",
    closest_balance.balance as "balance: _",
    bytecode as "bytecode: _",
    code_hash as "code_hash: _",
    static_slot_indexes as "static_slot_indexes: _",
    mapping_slot_indexes as "mapping_slot_indexes: _"
FROM accounts
JOIN closest_nonce ON true
JOIN closest_balance ON true
WHERE accounts.address = $1
