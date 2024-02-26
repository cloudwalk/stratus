INSERT INTO topics (topic, transaction_hash, transaction_idx, log_idx, topic_idx, block_number, block_hash)
SELECT * FROM UNNEST($1::bytea[], $2::bytea[], $3::int4[], $4::int4[], $5::int4[], $6::int8[], $7::bytea[])
