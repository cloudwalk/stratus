INSERT INTO topics (topic, transaction_hash, transaction_idx, log_idx, topic_idx, block_number, block_hash)
SELECT * FROM UNNEST($1::bytea[], $2::bytea[], $3::numeric[], $4::numeric[], $5::numeric[], $6::numeric[], $7::bytea[])
