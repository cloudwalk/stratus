CREATE TABLE public.accounts_temporary (
    id SERIAL PRIMARY KEY,
    address bytea UNIQUE NOT NULL,
    bytecode bytea,
    code_hash bytea NOT NULL,
    mapping_slot_indexes JSONB,
    static_slot_indexes JSONB,
    latest_balance numeric NOT NULL,
    latest_nonce numeric NOT NULL,
    creation_block numeric NOT NULL,
    previous_balance numeric,
    previous_nonce numeric,
    created_at timestamp(6) without time zone DEFAULT now() NOT NULL,
    updated_at timestamp(6) without time zone DEFAULT now() NOT NULL
);

CREATE TABLE public.account_slots_temporary (
    id SERIAL PRIMARY KEY,
    idx bytea NOT NULL,
    value bytea NOT NULL,
    account_address bytea NOT NULL,
    previous_value bytea,
    created_at timestamp(6) without time zone DEFAULT now() NOT NULL,
    updated_at timestamp(6) without time zone DEFAULT now() NOT NULL,
    UNIQUE (account_address, idx)
);

CREATE TABLE public.active_block_number (
   onerow_id bool PRIMARY KEY DEFAULT true,
   block_number numeric,
   CONSTRAINT onerow_uni CHECK (onerow_id)
);

INSERT INTO active_block_number(block_number) VALUES (NULL)
