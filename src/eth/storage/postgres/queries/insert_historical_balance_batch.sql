INSERT INTO historical_balances (address, balance, block_number)
SELECT * FROM UNNEST($1::bytea[], $2::numeric[], $3::int8[])
