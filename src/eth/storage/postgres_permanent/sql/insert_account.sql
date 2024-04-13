INSERT INTO accounts (
    address,
    latest_nonce,
    latest_balance,
    bytecode,
    code_hash,
    static_slot_indexes,
    mapping_slot_indexes,
    creation_block,
    previous_balance,
    previous_nonce
)
VALUES (
    $1, -- address,
    $2, -- latest_nonce,
    $3, -- latest_balance,
    $4, -- bytecode,
    $5, -- code_hash,
    $6, -- static_slot_indexes,
    $7, -- mapping_slot_indexes,
    $8, -- creation_block,
    $9, -- previous_balance,
    $10 -- previous_nonce
)
ON CONFLICT (address) DO
UPDATE
SET latest_nonce = EXCLUDED.latest_nonce,
    latest_balance = EXCLUDED.latest_balance
WHERE accounts.latest_nonce = excluded.previous_balance AND accounts.latest_balance = excluded.previous_balance
