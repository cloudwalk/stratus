INSERT INTO accounts
VALUES ($1, $2, $3, $4, $5)
ON CONFLICT (address) DO UPDATE
SET nonce = EXCLUDED.nonce,
    balance = EXCLUDED.balance,
    bytecode = EXCLUDED.bytecode
