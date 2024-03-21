SELECT
    idx as "index: _",
    value as "value: _"
FROM historical_slots
WHERE account_address = $1
AND idx = $2
AND block_number = (SELECT MAX(block_number)
                        FROM historical_slots
                        WHERE account_address = $1
                        AND idx = $2
                        AND block_number <= $3)
