INSERT INTO logs (
        address,
        data,
        transaction_hash,
        transaction_idx,
        log_idx,
        block_number,
        block_hash,
        topic0,
        topic1,
        topic2,
        topic3
    )
SELECT *
FROM UNNEST(
        $1::bytea [],
        $2::bytea [],
        $3::bytea [],
        $4::numeric [],
        $5::numeric [],
        $6::numeric [],
        $7::bytea [],
        $8::bytea [],
        $9::bytea [],
        $10::bytea [],
        $11::bytea []
    )
