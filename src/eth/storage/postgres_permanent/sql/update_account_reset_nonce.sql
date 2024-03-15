WITH latest_block_numbers AS (
    SELECT address, MAX(block_number) as block_number
    FROM historical_nonces
    GROUP BY address
),
latest_nonces AS (
    SELECT accounts.address, historical_nonces.nonce
    FROM accounts
    JOIN latest_block_numbers
    ON latest_block_numbers.address = accounts.address
    LEFT JOIN historical_nonces on accounts.address = historical_nonces.address
        AND latest_block_numbers.block_number = historical_nonces.block_number
)
UPDATE accounts
SET latest_nonce = latest_nonces.nonce
FROM latest_nonces
WHERE latest_nonces.address = accounts.address
    AND latest_nonces.nonce IS DISTINCT FROM accounts.latest_nonce
