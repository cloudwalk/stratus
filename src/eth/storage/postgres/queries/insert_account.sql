INSERT INTO accounts
VALUES ($1, $2, $3, $4, $5)
ON CONFLICT (address, block_number) DO UPDATE
SET nonce = EXCLUDED.nonce,
    balance = EXCLUDED.balance,
    bytecode = EXCLUDED.bytecode,
    block_number = EXCLUDED.block_number
