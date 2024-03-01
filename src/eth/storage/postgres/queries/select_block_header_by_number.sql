SELECT
    number as "number: _",
    hash as "hash: _",
    transactions_root as "transactions_root: _",
    gas_limit as "gas_limit: _",
    gas_used as "gas_used: _",
    logs_bloom as "bloom: _",
    timestamp_in_secs as "timestamp: _",
    parent_hash as "parent_hash: _",
    author as "author: _",
    extra_data as "extra_data: _",
    miner as "miner: _",
    difficulty as "difficulty: _",
    receipts_root as "receipts_root: _",
    uncle_hash as "uncle_hash: _",
    size as "size: _",
    state_root as "state_root: _",
    total_difficulty as "total_difficulty: _"
FROM blocks
WHERE number = $1
