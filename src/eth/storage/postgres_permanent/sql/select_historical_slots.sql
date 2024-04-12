SELECT idx as "index: _", value as "value: _"
FROM historical_slots
WHERE idx = ANY($1) AND account_address = $2 AND block_number = $3
