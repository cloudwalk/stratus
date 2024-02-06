WITH latest_block_numbers AS (
    SELECT account_address, idx, MAX(block_number) as block_number
    FROM historical_slots
    GROUP BY idx, account_address
),
latest_slots AS (
    SELECT account_slots.account_address, historical_slots.idx, historical_slots.value
    FROM account_slots
    JOIN latest_block_numbers
    ON latest_block_numbers.account_address = account_slots.account_address
        AND account_slots.idx = latest_block_numbers.idx
    LEFT JOIN historical_slots ON account_slots.account_address = historical_slots.account_address
        AND latest_block_numbers.block_number = historical_slots.block_number
        AND account_slots.idx = historical_slots.idx
)
UPDATE account_slots
SET value = latest_slots.value
FROM latest_slots
WHERE latest_slots.account_address = account_slots.account_address
    AND latest_slots.idx = account_slots.idx
    AND latest_slots.value IS DISTINCT FROM account_slots.value
