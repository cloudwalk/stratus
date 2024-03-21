SELECT
    address
    , data
    , transaction_hash
    , transaction_idx
    , log_idx
    , block_number
    , block_hash
FROM logs
WHERE true
