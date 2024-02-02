INSERT INTO accounts (address, latest_nonce, latest_balance, bytecode)
VALUES ($1, $2, $3, $4) ON CONFLICT (address) DO
UPDATE
SET latest_nonce = EXCLUDED.latest_nonce,
    latest_balance = EXCLUDED.latest_balance
