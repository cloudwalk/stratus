SELECT address,
    data,
    transaction_hash,
    transaction_idx,
    log_idx,
    block_number,
    block_hash,
    topic0,
    topic1,
    topic2,
    topic3
FROM logs
WHERE true
