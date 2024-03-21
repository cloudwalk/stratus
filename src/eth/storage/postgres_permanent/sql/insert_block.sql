INSERT INTO blocks(number, hash, transactions_root, gas, logs_bloom, timestamp_in_secs, parent_hash, code_hash, created_at)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, current_timestamp)
