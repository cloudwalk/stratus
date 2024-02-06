INSERT INTO account_slots VALUES ($1, $2, $3, $4)
ON CONFLICT (idx, account_address)
DO UPDATE SET value = EXCLUDED.value
WHERE account_slots.value = $5
