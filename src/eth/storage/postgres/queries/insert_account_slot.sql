INSERT INTO account_slots VALUES ($1, $2, $3, $4)
ON CONFLICT (idx, account_address, block_number)
DO UPDATE SET value = EXCLUDED.value
