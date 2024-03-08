WITH account_updates AS (
    SELECT *
    FROM
        unnest(
            $50::bytea [],
            $51::bytea [],
            $52::numeric [],
            $53::numeric [],
            $54::numeric [],
            $55::numeric [],
            $56::numeric []
        )
        AS t (
            address,
            bytecode,
            new_balance,
            new_nonce,
            creation_block,
            previous_balance,
            previous_nonce
        )
)

INSERT INTO accounts (
    address, bytecode, latest_balance, latest_nonce, creation_block, previous_balance, previous_nonce
)
SELECT
    address,
    bytecode,
    new_balance,
    new_nonce,
    creation_block,
    previous_balance,
    previous_nonce
FROM account_updates
ON CONFLICT (address) DO
UPDATE
SET latest_nonce = excluded.latest_nonce,
    latest_balance = excluded.latest_balance,
    previous_balance = excluded.previous_balance,
    previous_nonce = excluded.previous_nonce
WHERE accounts.latest_nonce = excluded.previous_nonce
AND accounts.latest_balance = excluded.previous_balance
