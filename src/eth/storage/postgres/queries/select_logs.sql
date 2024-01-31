SELECT
    address as "address:  "
    , data as "data:  "
    , transaction_hash as "transaction_hash: "
    , transaction_idx as "transaction_idx: _"
    , log_idx as "log_idx:  "
    , block_number as "block_number:  "
    , block_hash as "block_hash:  "
FROM logs
WHERE true
