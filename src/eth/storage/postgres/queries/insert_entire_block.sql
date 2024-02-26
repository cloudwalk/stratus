WITH block_insert AS (
    INSERT INTO blocks (
        number,
        hash,
        transactions_root,
        gas,
        logs_bloom,
        timestamp_in_secs,
        parent_hash,
        created_at
    )
    VALUES ($1, $2, $3, $4, $5, $6, $7, current_timestamp)
    RETURNING 1 as res
),

transaction_insert AS (
    INSERT INTO transactions (
        hash,
        signer_address,
        nonce,
        address_from,
        address_to,
        input,
        output,
        gas,
        gas_price,
        idx_in_block,
        block_number,
        block_hash,
        v,
        r,
        s,
        value,
        result
    )
    SELECT *
    FROM
        unnest(
            $8::bytea [],
            $9::bytea [],
            $10::numeric [],
            $11::bytea [],
            $12::bytea [],
            $13::bytea [],
            $14::bytea [],
            $15::numeric [],
            $16::numeric [],
            $17::int4 [],
            $18::int8 [],
            $19::bytea [],
            $20::bytea [],
            $21::bytea [],
            $22::bytea [],
            $23::numeric [],
            $24::text []
        )
    RETURNING 1 as res
),

log_insert AS (
    INSERT INTO logs (
        address,
        data,
        transaction_hash,
        transaction_idx,
        log_idx,
        block_number,
        block_hash
    )
    SELECT *
    FROM
        unnest(
            $25::bytea [],
            $26::bytea [],
            $27::bytea [],
            $28::int4 [],
            $29::int4 [],
            $30::int8 [],
            $31::bytea []
        )
    RETURNING 1 as res
),

topic_insert AS (
    INSERT INTO topics (
        topic,
        transaction_hash,
        transaction_idx,
        log_idx,
        topic_idx,
        block_number,
        block_hash
    )
    SELECT *
    FROM
        unnest(
            $32::bytea [],
            $33::bytea [],
            $34::int4 [],
            $35::int4 [],
            $36::int4 [],
            $37::int8 [],
            $38::bytea []
        )
    RETURNING 1 as res
),

account_insert AS (
    WITH account_updates AS (
        SELECT *
        FROM
            unnest(
                $39::bytea [],
                $40::bytea [],
                $41::numeric [],
                $42::numeric [],
                $43::int8 [],
                $44::numeric [],
                $45::numeric []
            )
            AS t (
                address,
                bytecode,
                new_balance,
                new_nonce,
                creation_block,
                original_balance,
                original_nonce
            )
    )

    INSERT INTO accounts (
        address, bytecode, latest_balance, latest_nonce, creation_block
    )
    SELECT
        address,
        bytecode,
        new_balance,
        new_nonce,
        creation_block
    FROM account_updates
    ON CONFLICT (address) DO
    UPDATE
    SET latest_nonce = excluded.latest_nonce,
    latest_balance = excluded.latest_balance
    WHERE accounts.latest_nonce
    = (
        SELECT original_nonce
        FROM account_updates
        WHERE account_updates.address = excluded.address
    )
    AND accounts.latest_balance
    = (
        SELECT original_balance
        FROM account_updates
        WHERE account_updates.address = excluded.address
    )
    RETURNING 1 as res
),

slot_insert AS (
    WITH slot_updates AS (
        SELECT *
        FROM
            unnest(
                $46::bytea [],
                $47::bytea [],
                $48::bytea [],
                $49::int8 [],
                $50::bytea []
            )
            AS t (idx, value, account_address, creation_block, original_value)
    )

    INSERT INTO account_slots (idx, value, account_address, creation_block)
    SELECT
        idx,
        value,
        account_address,
        creation_block
    FROM slot_updates
    ON CONFLICT (idx, account_address) DO
    UPDATE
    SET value = excluded.value
    WHERE account_slots.value = (
        SELECT original_value
        FROM slot_updates
        WHERE
            slot_updates.idx = excluded.idx
            AND slot_updates.account_address = excluded.account_address
    )
    RETURNING 1 as res
),

historical_nonce_insert AS (
    INSERT INTO historical_nonces (address, nonce, block_number)
    SELECT * FROM unnest($51::bytea [], $52::numeric [], $53::int8 [])
    RETURNING 1 as res
),

historical_balance_insert AS (
    INSERT INTO historical_balances (address, balance, block_number)
    SELECT * FROM unnest($54::bytea [], $55::numeric [], $56::int8 [])
    RETURNING 1 as res
),

historical_slots_insert AS (
    INSERT INTO historical_slots (idx, value, account_address, block_number)
    SELECT *
    FROM unnest($57::bytea [], $58::bytea [], $59::bytea [], $60::int8 [])
    RETURNING 1 as res
)

SELECT Sum(count) as affected_rows FROM
    ((SELECT count(*) AS count FROM block_insert)
        UNION ALL
        SELECT count(*) AS count FROM transaction_insert
        UNION ALL
        SELECT count(*) AS count FROM log_insert
        UNION ALL
        SELECT count(*) AS count FROM topic_insert
        UNION ALL
        SELECT count(*) AS count FROM account_insert
        UNION ALL
        SELECT count(*) AS count FROM slot_insert
        UNION ALL
        SELECT count(*) AS count FROM historical_balance_insert
        UNION ALL
        SELECT count(*) AS count FROM historical_nonce_insert
        UNION ALL
        SELECT count(*) AS count FROM historical_slots_insert);
