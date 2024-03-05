WITH account_updates
AS (SELECT *
    FROM UNNEST($1::bytea[], $2::bytea[], $3::numeric[], $4::numeric[], $5::numeric[], $6::numeric[], $7::numeric[])
    AS t(address, bytecode, new_balance, new_nonce, creation_block, original_balance, original_nonce)
)
INSERT INTO accounts (address, bytecode, latest_balance, latest_nonce, creation_block)
SELECT address, bytecode, new_balance, new_nonce, creation_block
FROM account_updates
ON CONFLICT (address) DO
UPDATE
SET latest_nonce = EXCLUDED.latest_nonce,
    latest_balance = EXCLUDED.latest_balance
WHERE accounts.latest_nonce = (SELECT original_nonce FROM account_updates WHERE account_updates.address=accounts.address)
    AND accounts.latest_balance = (SELECT original_balance FROM account_updates WHERE account_updates.address=accounts.address)
