SELECT
    idx as "index: _",
    value as "value: _",
    account_address as "address: _",
    creation_block as "block: _"
FROM account_slots
WHERE creation_block >= $1;
