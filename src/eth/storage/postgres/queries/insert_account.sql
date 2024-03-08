INSERT INTO accounts (
    address, bytecode, latest_balance, latest_nonce, creation_block, previous_balance, previous_nonce
)
VALUES ($1, $2, $3, $4, $5, $6, $7)
ON CONFLICT (address) DO
UPDATE
SET latest_nonce = EXCLUDED.latest_nonce,
    latest_balance = EXCLUDED.latest_balance
WHERE accounts.latest_nonce = excluded.previous_balance AND accounts.latest_balance = excluded.previous_balance
