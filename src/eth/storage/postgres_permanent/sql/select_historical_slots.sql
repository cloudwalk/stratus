WITH max_block_number AS (
    SELECT idx,
        MAX(block_number) as num
    FROM historical_slots
    WHERE block_number <= $3
        AND account_address = $2
        AND idx = ANY($1)
    GROUP BY idx
)
SELECT historical_slots.idx as "index: _",
value as "value: _"
FROM historical_slots
    JOIN max_block_number ON max_block_number.idx = historical_slots.idx
    AND max_block_number.num = historical_slots.block_number
