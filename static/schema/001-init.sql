
SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

--
-- Name: grant_sequence_privileges(); Type: PROCEDURE; Schema: public; Owner: -
--

CREATE PROCEDURE public.grant_sequence_privileges()
    LANGUAGE plpgsql
    AS $$
DECLARE
    rec record;
BEGIN
    FOR rec IN (
        WITH users AS (
            SELECT
                r.oid as role_id,
                r.rolname AS username
            FROM pg_roles r
        ),
        base AS (
            SELECT
                nsp.nspname as schema_name,
                seq.relname as seq_name,
                seq.oid AS seq_oid,
                tbl.oid as table_oid
            FROM pg_class seq
            JOIN pg_depend dep ON seq.relfilenode = dep.objid
            JOIN pg_class tbl  ON dep.refobjid = tbl.relfilenode
            JOIN pg_namespace nsp ON nsp.oid = seq.relnamespace
            WHERE
                nsp.nspname NOT IN ('pg_catalog', 'information_schema')
                AND seq.relkind = 'S'
                AND tbl.relkind = 'r'
        )
        SELECT
            u.username,
            b.schema_name,
            b.seq_name
        FROM users u
        JOIN base b ON has_table_privilege(u.role_id, b.table_oid, 'INSERT')
        WHERE
            (NOT has_sequence_privilege(u.role_id, b.seq_oid, 'USAGE')
             OR NOT has_sequence_privilege(u.role_id, b.seq_oid, 'SELECT'))
    )
    LOOP
        EXECUTE FORMAT('GRANT USAGE, SELECT ON SEQUENCE %I.%I TO %I', rec.schema_name, rec.seq_name, rec.username);
    END LOOP;
END;
$$;


SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: account_slots; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.account_slots (
    id bigint NOT NULL,
    idx bytea NOT NULL,
    value bytea NOT NULL,
    account_address bytea NOT NULL,
    creation_block numeric NOT NULL,
    previous_value bytea,
    created_at timestamp(6) without time zone DEFAULT now() NOT NULL,
    updated_at timestamp(6) without time zone DEFAULT now() NOT NULL
);


--
-- Name: TABLE account_slots; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON TABLE public.account_slots IS 'Blockchain contract state';


--
-- Name: account_slots_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.account_slots_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: account_slots_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.account_slots_id_seq OWNED BY public.account_slots.id;


--
-- Name: accounts; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.accounts (
    id bigint NOT NULL,
    address bytea NOT NULL,
    bytecode bytea,
    code_hash bytea NOT NULL,
    slot_indexes_mapping_access bytea[],
    slot_indexes_static_access bytea[],
    latest_balance numeric NOT NULL,
    latest_nonce numeric NOT NULL,
    creation_block numeric NOT NULL,
    previous_balance numeric,
    previous_nonce numeric,

    created_at timestamp(6) without time zone DEFAULT now() NOT NULL,
    updated_at timestamp(6) without time zone DEFAULT now() NOT NULL
);


--
-- Name: TABLE accounts; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON TABLE public.accounts IS 'Blockchain wallets';


--
-- Name: accounts_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.accounts_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: accounts_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.accounts_id_seq OWNED BY public.accounts.id;


--
-- Name: ar_internal_metadata; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.ar_internal_metadata (
    key character varying NOT NULL,
    value character varying,
    created_at timestamp(6) without time zone NOT NULL,
    updated_at timestamp(6) without time zone NOT NULL
);


--
-- Name: blocks; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.blocks (
    id bigint NOT NULL,
    number numeric NOT NULL,
    hash bytea NOT NULL,
    transactions_root bytea NOT NULL,
    gas_limit numeric NOT NULL,
    gas_used numeric NOT NULL,
    logs_bloom bytea NOT NULL,
    timestamp_in_secs numeric NOT NULL,
    parent_hash bytea NOT NULL,
    author bytea NOT NULL,
    extra_data bytea NOT NULL,
    miner bytea NOT NULL,
    difficulty numeric NOT NULL,
    receipts_root bytea NOT NULL,
    uncle_hash bytea NOT NULL,
    size numeric NOT NULL,
    state_root bytea NOT NULL,
    total_difficulty numeric NOT NULL,
    nonce bytea NOT NULL,
    created_at timestamp(6) without time zone DEFAULT now() NOT NULL,
    updated_at timestamp(6) without time zone DEFAULT now() NOT NULL
);


--
-- Name: TABLE blocks; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON TABLE public.blocks IS 'Stores every block of the blockchain';


--
-- Name: blocks_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.blocks_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: blocks_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.blocks_id_seq OWNED BY public.blocks.id;


--
-- Name: historical_balances; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.historical_balances (
    id bigint NOT NULL,
    address bytea NOT NULL,
    balance numeric NOT NULL,
    block_number numeric NOT NULL,
    created_at timestamp(6) without time zone DEFAULT now() NOT NULL,
    updated_at timestamp(6) without time zone DEFAULT now() NOT NULL
);


--
-- Name: TABLE historical_balances; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON TABLE public.historical_balances IS 'Historical balances of wallets';


--
-- Name: historical_balances_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.historical_balances_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: historical_balances_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.historical_balances_id_seq OWNED BY public.historical_balances.id;


--
-- Name: historical_nonces; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.historical_nonces (
    id bigint NOT NULL,
    address bytea NOT NULL,
    nonce numeric NOT NULL,
    block_number numeric NOT NULL,
    created_at timestamp(6) without time zone DEFAULT now() NOT NULL,
    updated_at timestamp(6) without time zone DEFAULT now() NOT NULL
);


--
-- Name: TABLE historical_nonces; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON TABLE public.historical_nonces IS 'Historical nonce of wallets';


--
-- Name: historical_nonces_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.historical_nonces_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: historical_nonces_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.historical_nonces_id_seq OWNED BY public.historical_nonces.id;


--
-- Name: historical_slots; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.historical_slots (
    id bigint NOT NULL,
    idx bytea NOT NULL,
    value bytea NOT NULL,
    block_number numeric NOT NULL,
    account_address bytea NOT NULL,
    created_at timestamp(6) without time zone DEFAULT now() NOT NULL,
    updated_at timestamp(6) without time zone DEFAULT now() NOT NULL
);


--
-- Name: TABLE historical_slots; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON TABLE public.historical_slots IS 'Blockchain contracts state history';


--
-- Name: historical_slots_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.historical_slots_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: historical_slots_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.historical_slots_id_seq OWNED BY public.historical_slots.id;


--
-- Name: logs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.logs (
    id bigint NOT NULL,
    address bytea NOT NULL,
    data bytea NOT NULL,
    transaction_hash bytea NOT NULL,
    transaction_idx numeric NOT NULL,
    log_idx numeric NOT NULL,
    block_number numeric NOT NULL,
    block_hash bytea NOT NULL,
    created_at timestamp(6) without time zone DEFAULT now() NOT NULL,
    updated_at timestamp(6) without time zone DEFAULT now() NOT NULL
);


--
-- Name: TABLE logs; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON TABLE public.logs IS 'Blockchain transaction logs';


--
-- Name: logs_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.logs_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: logs_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.logs_id_seq OWNED BY public.logs.id;


--
-- Name: schema_migrations; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.schema_migrations (
    version character varying NOT NULL
);


--
-- Name: topics; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.topics (
    id bigint NOT NULL,
    topic bytea NOT NULL,
    transaction_hash bytea NOT NULL,
    transaction_idx numeric NOT NULL,
    log_idx numeric NOT NULL,
    topic_idx numeric NOT NULL,
    block_number numeric NOT NULL,
    block_hash bytea NOT NULL,
    created_at timestamp(6) without time zone DEFAULT now() NOT NULL,
    updated_at timestamp(6) without time zone DEFAULT now() NOT NULL
);


--
-- Name: TABLE topics; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON TABLE public.topics IS 'Blockchain log topics';


--
-- Name: topics_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.topics_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: topics_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.topics_id_seq OWNED BY public.topics.id;


--
-- Name: transactions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.transactions (
    id bigint NOT NULL,
    hash bytea NOT NULL,
    signer_address bytea NOT NULL,
    nonce numeric NOT NULL,
    address_from bytea NOT NULL,
    address_to bytea,
    input bytea NOT NULL,
    output bytea NOT NULL,
    gas numeric NOT NULL,
    gas_price numeric NOT NULL,
    idx_in_block numeric NOT NULL,
    block_number numeric NOT NULL,
    block_hash bytea NOT NULL,
    v bytea NOT NULL,
    r bytea NOT NULL,
    s bytea NOT NULL,
    value numeric NOT NULL,
    result text NOT NULL,
    created_at timestamp(6) without time zone DEFAULT now() NOT NULL,
    updated_at timestamp(6) without time zone DEFAULT now() NOT NULL
);


--
-- Name: TABLE transactions; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON TABLE public.transactions IS 'Blockchain transactions';


--
-- Name: transactions_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.transactions_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: transactions_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.transactions_id_seq OWNED BY public.transactions.id;


--
-- Name: account_slots id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.account_slots ALTER COLUMN id SET DEFAULT nextval('public.account_slots_id_seq'::regclass);


--
-- Name: accounts id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.accounts ALTER COLUMN id SET DEFAULT nextval('public.accounts_id_seq'::regclass);


--
-- Name: blocks id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.blocks ALTER COLUMN id SET DEFAULT nextval('public.blocks_id_seq'::regclass);


--
-- Name: historical_balances id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.historical_balances ALTER COLUMN id SET DEFAULT nextval('public.historical_balances_id_seq'::regclass);


--
-- Name: historical_nonces id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.historical_nonces ALTER COLUMN id SET DEFAULT nextval('public.historical_nonces_id_seq'::regclass);


--
-- Name: historical_slots id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.historical_slots ALTER COLUMN id SET DEFAULT nextval('public.historical_slots_id_seq'::regclass);


--
-- Name: logs id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.logs ALTER COLUMN id SET DEFAULT nextval('public.logs_id_seq'::regclass);


--
-- Name: topics id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.topics ALTER COLUMN id SET DEFAULT nextval('public.topics_id_seq'::regclass);


--
-- Name: transactions id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.transactions ALTER COLUMN id SET DEFAULT nextval('public.transactions_id_seq'::regclass);


--
-- Name: account_slots account_slots_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.account_slots
    ADD CONSTRAINT account_slots_pkey PRIMARY KEY (id);


--
-- Name: accounts accounts_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.accounts
    ADD CONSTRAINT accounts_pkey PRIMARY KEY (id);


--
-- Name: ar_internal_metadata ar_internal_metadata_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.ar_internal_metadata
    ADD CONSTRAINT ar_internal_metadata_pkey PRIMARY KEY (key);


--
-- Name: blocks blocks_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.blocks
    ADD CONSTRAINT blocks_pkey PRIMARY KEY (id);


--
-- Name: historical_balances historical_balances_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.historical_balances
    ADD CONSTRAINT historical_balances_pkey PRIMARY KEY (id);


--
-- Name: historical_nonces historical_nonces_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.historical_nonces
    ADD CONSTRAINT historical_nonces_pkey PRIMARY KEY (id);


--
-- Name: historical_slots historical_slots_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.historical_slots
    ADD CONSTRAINT historical_slots_pkey PRIMARY KEY (id);


--
-- Name: logs logs_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.logs
    ADD CONSTRAINT logs_pkey PRIMARY KEY (id);


--
-- Name: schema_migrations schema_migrations_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.schema_migrations
    ADD CONSTRAINT schema_migrations_pkey PRIMARY KEY (version);


--
-- Name: topics topics_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.topics
    ADD CONSTRAINT topics_pkey PRIMARY KEY (id);


--
-- Name: transactions transactions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.transactions
    ADD CONSTRAINT transactions_pkey PRIMARY KEY (id);


--
-- Name: index_account_slots_on_idx_and_account_address; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX index_account_slots_on_idx_and_account_address ON public.account_slots USING btree (idx, account_address);


--
-- Name: index_accounts_on_address; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX index_accounts_on_address ON public.accounts USING btree (address);


--
-- Name: index_blocks_on_hash; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX index_blocks_on_hash ON public.blocks USING btree (hash);


--
-- Name: index_blocks_on_number; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX index_blocks_on_number ON public.blocks USING btree (number);


--
-- Name: index_blocks_on_parent_hash; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX index_blocks_on_parent_hash ON public.blocks USING btree (parent_hash);


--
-- Name: index_historical_balances_on_address_and_block_number; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX index_historical_balances_on_address_and_block_number ON public.historical_balances USING btree (address, block_number);


--
-- Name: index_historical_nonces_on_address_and_block_number; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX index_historical_nonces_on_address_and_block_number ON public.historical_nonces USING btree (address, block_number);


--
-- Name: index_historical_slots_on_idx_and_address_and_block_number; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX index_historical_slots_on_idx_and_address_and_block_number ON public.historical_slots USING btree (idx, account_address, block_number);


--
-- Name: index_logs_on_block_hash_and_log_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX index_logs_on_block_hash_and_log_idx ON public.logs USING btree (block_hash, log_idx);


--
-- Name: index_topics_on_block_hash_and_log_idx_and_topic_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX index_topics_on_block_hash_and_log_idx_and_topic_idx ON public.topics USING btree (block_hash, log_idx, topic_idx);


--
-- Name: index_transactions_on_hash; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX index_transactions_on_hash ON public.transactions USING btree (hash);


--
-- PostgreSQL database dump complete
--



--- XXX temporary
CREATE TABLE public.neo_blocks (
    block_number BIGINT PRIMARY KEY,
    block_hash BYTEA NOT NULL,
    block JSONB NOT NULL,
    account_changes JSONB NOT NULL,
    created_at TIMESTAMP WITHOUT TIME ZONE DEFAULT now()
) PARTITION BY RANGE (block_number);

CREATE TABLE public.neo_accounts (
    block_number BIGINT NOT NULL,
    address BYTEA NOT NULL,
    bytecode BYTEA,
    code_hash bytea NOT NULL,
    balance NUMERIC NOT NULL,
    nonce NUMERIC NOT NULL,
    created_at TIMESTAMP WITHOUT TIME ZONE DEFAULT now(),
    PRIMARY KEY (address, block_number)
) PARTITION BY RANGE (block_number);

CREATE TABLE public.neo_account_slots (
    block_number BIGINT NOT NULL,
    slot_index BYTEA NOT NULL,
    account_address BYTEA NOT NULL,
    value BYTEA NOT NULL,
    created_at TIMESTAMP WITHOUT TIME ZONE DEFAULT now(),
    PRIMARY KEY (account_address, slot_index, block_number)
) PARTITION BY RANGE (block_number);

-- Create indexes on the parent tables
CREATE INDEX neo_blocks_block_number_desc_idx ON public.neo_blocks (block_number DESC);
CREATE INDEX neo_accounts_block_number_desc_idx ON public.neo_accounts (block_number DESC);
CREATE INDEX neo_account_slots_block_number_desc_idx ON public.neo_account_slots (block_number DESC);

DO $$
DECLARE
    start_block BIGINT := 0;
    end_block BIGINT := 30999999;
    partition_suffix TEXT;
BEGIN
    FOR i IN 1..7 LOOP -- this will create partitions until 72M blocks
        partition_suffix := i::TEXT; -- Convert loop index to text for suffix

        -- Create partitions for public.neo_blocks
        EXECUTE 'CREATE TABLE public.neo_blocks_p' || partition_suffix || ' PARTITION OF public.neo_blocks FOR VALUES FROM (' || start_block || ') TO (' || end_block || ')';
        EXECUTE 'CREATE INDEX neo_blocks_p' || partition_suffix || '_block_number_desc_idx ON public.neo_blocks_p' || partition_suffix || ' (block_number DESC)';

        -- Create partitions for public.neo_accounts
        EXECUTE 'CREATE TABLE public.neo_accounts_p' || partition_suffix || ' PARTITION OF public.neo_accounts FOR VALUES FROM (' || start_block || ') TO (' || end_block || ')';
        EXECUTE 'CREATE INDEX neo_accounts_p' || partition_suffix || '_block_number_desc_idx ON public.neo_accounts_p' || partition_suffix || ' (block_number DESC)';

        -- Create partitions for public.neo_account_slots
        EXECUTE 'CREATE TABLE public.neo_account_slots_p' || partition_suffix || ' PARTITION OF public.neo_account_slots FOR VALUES FROM (' || start_block || ') TO (' || end_block || ')';
        EXECUTE 'CREATE INDEX neo_account_slots_p' || partition_suffix || '_block_number_desc_idx ON public.neo_account_slots_p' || partition_suffix || ' (block_number DESC)';

        -- Update range for the next partition, incrementing by 5M each time
        start_block := end_block;
        end_block := end_block + 7862400; -- Increment by a quarter considering 1 block = 1 second
    END LOOP;
END$$;

CREATE TABLE public.neo_transactions (
    hash BYTEA NOT NULL,
    hash_partition SMALLINT NOT NULL,
    block_number BIGINT NOT NULL,
    transaction_data JSONB NOT NULL,
    created_at TIMESTAMP WITHOUT TIME ZONE DEFAULT now(),
    PRIMARY KEY (hash, hash_partition)
) PARTITION BY LIST (hash_partition);
CREATE INDEX idx_neo_transactions_hash ON public.neo_transactions (hash);

CREATE TABLE public.neo_logs (
    hash BYTEA NOT NULL,
    hash_partition SMALLINT NOT NULL,
    block_number BIGINT NOT NULL,
    log_idx NUMERIC NOT NULL,
    address BYTEA NOT NULL,
    log_data JSONB NOT NULL,
    created_at TIMESTAMP WITHOUT TIME ZONE DEFAULT now(),
    PRIMARY KEY (hash, block_number, log_idx, hash_partition)
) PARTITION BY LIST (hash_partition);
CREATE INDEX idx_neo_logs_hash ON public.neo_logs (hash);

DO $$
DECLARE
    partition_id INT;
BEGIN
    FOR partition_id IN 0..9 LOOP
        -- Create partitions for public.neo_transactions
        EXECUTE format('CREATE TABLE public.neo_transactions_%s PARTITION OF public.neo_transactions FOR VALUES IN (%L);', partition_id, partition_id);
        EXECUTE format('CREATE INDEX ON public.neo_transactions_%s (hash_partition);', partition_id);
        EXECUTE format('CREATE INDEX ON public.neo_transactions_%s (hash);', partition_id);
        EXECUTE format('CREATE INDEX ON public.neo_transactions_%s (block_number);', partition_id);

        -- Create partitions for public.neo_logs
        EXECUTE format('CREATE TABLE public.neo_logs_%s PARTITION OF public.neo_logs FOR VALUES IN (%L);', partition_id, partition_id);
        EXECUTE format('CREATE INDEX ON public.neo_logs_%s (hash_partition);', partition_id);
        EXECUTE format('CREATE INDEX ON public.neo_logs_%s (hash);', partition_id);
        EXECUTE format('CREATE INDEX ON public.neo_logs_%s (block_number);', partition_id);
    END LOOP;
END$$;

-- XXX END



SET search_path TO "$user", public;

INSERT INTO "schema_migrations" (version) VALUES
('20240222172433'),
('20240226173425'),
('20240226180139'),
('20240226180236'),
('20240229181559'),
('20240229181850'),
('20240229182108'),
('20240229183514'),
('20240229183643'),
('20240311224030');

