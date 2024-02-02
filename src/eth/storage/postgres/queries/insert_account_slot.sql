INSERT INTO historical_slots VALUES ($1, $2, $3)
ON CONFLICT (idx, account_address)
DO UPDATE SET value = EXCLUDED.value
