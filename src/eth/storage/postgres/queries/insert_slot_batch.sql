WITH slot_updates
AS (SELECT *
    FROM UNNEST($1::bytea[], $2::bytea[], $3::bytea[], $4::int8[], $5::bytea[])
    AS t(idx, value, account_address, creation_block, original_value)
)
INSERT INTO account_slots (idx, value, account_address, creation_block)
SELECT idx, value, account_address, creation_block
FROM slot_updates
ON CONFLICT (idx, account_address) DO
UPDATE
SET value = EXCLUDED.value
WHERE account_slots.value = (
    SELECT original_value
    FROM slot_updates
    WHERE slot_updates.idx = EXCLUDED.idx
        AND slot_updates.account_address = EXCLUDED.account_address)
