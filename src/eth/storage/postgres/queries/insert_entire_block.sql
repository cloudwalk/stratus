WITH block_insert AS (
    INSERT INTO blocks (
        number,
        hash,
        transactions_root,
        gas_limit,
        gas_used,
        logs_bloom,
        timestamp_in_secs,
        parent_hash,
        author,
        extra_data,
        miner,
        difficulty,
        receipts_root,
        uncle_hash,
        size,
        state_root,
        total_difficulty,
        nonce,
        created_at
    )
    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, current_timestamp)
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
            $19::bytea [],
            $20::bytea [],
            $21::numeric [],
            $22::bytea [],
            $23::bytea [],
            $24::bytea [],
            $25::bytea [],
            $26::numeric [],
            $27::numeric [],
            $28::numeric [],
            $29::numeric [],
            $30::bytea [],
            $31::bytea [],
            $32::bytea [],
            $33::bytea [],
            $34::numeric [],
            $35::text []
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
            $36::bytea [],
            $37::bytea [],
            $38::bytea [],
            $39::numeric [],
            $40::numeric [],
            $41::numeric [],
            $42::bytea []
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
            $43::bytea [],
            $44::bytea [],
            $45::numeric [],
            $46::numeric [],
            $47::numeric [],
            $48::numeric [],
            $49::bytea []
        )
    RETURNING 1 as res
),

account_insert AS (
    WITH account_updates AS (
        SELECT *
        FROM
            unnest(
                $50::bytea [],
                $51::bytea [],
                $52::numeric [],
                $53::numeric [],
                $54::numeric [],
                $55::numeric [],
                $56::numeric []
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
                $57::bytea [],
                $58::bytea [],
                $59::bytea [],
                $60::numeric [],
                $61::bytea []
            )
            AS t (idx, value, account_address, block_number, original_value)
    )

    INSERT INTO account_slots (idx, value, previous_value, account_address, creation_block)
    SELECT
        idx,
        value,
        original_value,
        account_address,
        block_number
    FROM slot_updates
    ON CONFLICT (idx, account_address) DO
    UPDATE
    SET value = excluded.value,
        previous_value = excluded.previous_value
    WHERE account_slots.value = excluded.previous_value
    RETURNING 1 as res
),

historical_nonce_insert AS (
    INSERT INTO historical_nonces (address, nonce, block_number)
    SELECT * FROM unnest($62::bytea [], $63::numeric [], $64::numeric [])
    RETURNING 1 as res
),

historical_balance_insert AS (
    INSERT INTO historical_balances (address, balance, block_number)
    SELECT * FROM unnest($65::bytea [], $66::numeric [], $67::numeric [])
    RETURNING 1 as res
),

historical_slots_insert AS (
    INSERT INTO historical_slots (idx, value, account_address, block_number)
    SELECT *
    FROM unnest($68::bytea [], $69::bytea [], $70::bytea [], $71::numeric [])
    RETURNING 1 as res
)

SELECT modified_accounts, modified_slots FROM
(SELECT count(*) AS modified_accounts FROM account_insert)
CROSS JOIN
(SELECT count(*) AS modified_slots FROM slot_insert);
