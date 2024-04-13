SELECT idx as "index: _", value as "value: _"
FROM account_slots
WHERE idx = ANY($1) AND account_address = $2
