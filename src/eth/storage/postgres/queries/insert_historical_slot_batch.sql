INSERT INTO historical_slots (idx, value, account_address, block_number)
SELECT * FROM UNNEST($1::bytea[], $2::bytea[], $3::bytea[],$4::numeric[])
