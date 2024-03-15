SELECT
    idx as "index: _",
    value as "value: _"
FROM account_slots
WHERE account_address = $1
AND idx = $2
