SELECT
    address as "address: _"
    ,data as "data: _"
    ,transaction_hash as "transaction_hash: _"
    ,transaction_idx as "transaction_idx: _"
    ,log_idx as "log_idx: _"
    ,block_number as "block_number: _"
    ,block_hash as "block_hash: _"
    ,topic0 as "topic0: _"
    ,topic1 as "topic1: _"
    ,topic2 as "topic2: _"
    ,topic3 as "topic3: _"
FROM logs
WHERE transaction_hash = $1
