INSERT INTO account_slots(idx, value, previous_value, account_address, creation_block)
VALUES ($1, $2, $3, $4, $5)
ON CONFLICT (idx, account_address)
DO UPDATE SET value = EXCLUDED.value
WHERE account_slots.value = excluded.previous_value
