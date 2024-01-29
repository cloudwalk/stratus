SELECT
    number as "number: _"
    ,hash as "hash: _"
    ,transactions_root as "transactions_root: _"
    ,gas as "gas: _"
    ,logs_bloom as "bloom: _"
    ,timestamp_in_secs as "timestamp_in_secs: _"
FROM blocks
WHERE hash = $1
