SELECT
    number as "number: _"
    ,hash as "hash: _"
    ,transactions_root as "transactions_root: _"
    ,gas as "gas: _"
    ,logs_bloom as "bloom: _"
    ,timestamp_in_secs as "timestamp: _"
    ,parent_hash as "parent_hash: _"
FROM blocks
WHERE number = $1