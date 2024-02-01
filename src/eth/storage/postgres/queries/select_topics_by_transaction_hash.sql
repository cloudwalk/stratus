SELECT
    topic as "topic: _"
    ,transaction_hash as "transaction_hash: _"
    ,transaction_idx as "transaction_idx: _"
    ,log_idx as "log_idx: _"
    ,block_number as "block_number: _"
    ,block_hash as "block_hash: _"
FROM topics
WHERE transaction_hash = $1
ORDER BY topic_idx
