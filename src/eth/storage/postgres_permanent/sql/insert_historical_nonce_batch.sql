INSERT INTO historical_nonces (address, nonce, block_number)
SELECT * FROM UNNEST($1::bytea[], $2::numeric[], $3::numeric[])
