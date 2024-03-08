WITH slot_updates AS (
    SELECT *
    FROM
        unnest(
            $57::bytea [],
            $58::bytea [],
            $59::bytea [],
            $60::numeric [],
            $61::bytea []
        )
        AS t (idx, value, account_address, creation_block, original_value)
)

INSERT INTO account_slots (idx, value, previous_value, account_address, creation_block)
SELECT
    idx,
    value,
    original_value,
    account_address,
    creation_block
FROM slot_updates
ON CONFLICT (idx, account_address) DO
UPDATE
SET value = excluded.value,
    previous_value = excluded.previous_value
WHERE account_slots.value = excluded.previous_value
