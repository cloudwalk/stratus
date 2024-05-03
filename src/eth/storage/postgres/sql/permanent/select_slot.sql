SELECT value
FROM account_slots
WHERE account_address = $1
AND idx = $2
