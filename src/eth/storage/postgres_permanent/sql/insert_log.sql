INSERT INTO logs (
        address,
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
    )
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
