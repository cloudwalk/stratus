SELECT
    topic as "topic: _"
    ,transaction_hash as "transaction_hash: _"
    ,transaction_idx as "transaction_idx: _"
    ,log_idx as "log_idx: _"
    ,block_number as "block_number: _"
    ,block_hash as "block_hash: _"
FROM topics
WHERE block_hash = $1 AND log_idx = $2
ORDER BY topic_idx
