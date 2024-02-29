SELECT
    idx as "index!: _",
    value as "value!: _",
    account_address as "address!: _",
    block_number as "block_number!: _"
FROM
    (SELECT
        setseed($1),
        null as "idx",
        null as "value",
        null as "account_address",
        null as "block_number"
    UNION ALL
    SELECT
        null,
        idx,
        value,
        account_address,
        block_number
    FROM historical_slots
    WHERE block_number >= $2 AND block_number <= $3
    OFFSET 1
    )
ORDER BY random()
LIMIT $4;
