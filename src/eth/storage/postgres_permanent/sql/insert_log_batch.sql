INSERT INTO logs (address, data, transaction_hash, transaction_idx, log_idx, block_number, block_hash)
SELECT * FROM UNNEST($1::bytea[], $2::bytea[], $3::bytea[], $4::numeric[], $5::numeric[], $6::numeric[], $7::bytea[])
