INSERT INTO transactions (hash, signer_address, nonce, address_from,  address_to, input, output, gas, gas_price, idx_in_block, block_number, block_hash, v, r, s, value, result)
SELECT * FROM UNNEST($1::bytea[], $2::bytea[], $3::numeric[], $4::bytea[], $5::bytea[], $6::bytea[], $7::bytea[], $8::numeric[], $9::numeric[], $10::int4[], $11::int8[], $12::bytea[], $13::bytea[], $14::bytea[], $15::bytea[], $16::numeric[], $17::text[])
