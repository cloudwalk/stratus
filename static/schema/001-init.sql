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
-- Name: public; Type: SCHEMA; Schema: -; Owner: -
--

-- *not* creating schema, since initdb creates it


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

--- XXX temporary
CREATE TABLE public.neo_blocks (
    block_number BIGINT PRIMARY KEY,
    block_hash BYTEA NOT NULL,
    block JSONB NOT NULL,
    account_changes JSONB NOT NULL,
    created_at TIMESTAMP WITHOUT TIME ZONE DEFAULT now()
);

CREATE TABLE public.neo_accounts (
    block_number BIGINT NOT NULL,
    address BYTEA NOT NULL,
    bytecode BYTEA,
<<<<<<< HEAD
    code_hash BYTEA NOT NULL CHECK (LENGTH(code_hash) = 32), -- if bytecode is null code_hash is hash of empty string
    latest_balance NUMERIC NOT NULL CHECK (latest_balance >= 0),
    latest_nonce NUMERIC NOT NULL CHECK (latest_nonce >= 0),
    previous_balance NUMERIC CHECK (latest_balance >= 0),
    previous_nonce NUMERIC CHECK (latest_balance >= 0),
    creation_block  NUMERIC NOT NULL REFERENCES blocks (number) ON DELETE CASCADE,
    PRIMARY KEY (address)
);

CREATE TABLE IF NOT EXISTS historical_balances (
    address BYTEA NOT NULL CHECK (LENGTH(address) = 20) REFERENCES accounts (address) ON DELETE CASCADE,
    balance NUMERIC NOT NULL CHECK (balance >= 0),
    block_number NUMERIC NOT NULL REFERENCES blocks (number) ON DELETE CASCADE,
=======
    balance NUMERIC NOT NULL,
    nonce NUMERIC NOT NULL,
    created_at TIMESTAMP WITHOUT TIME ZONE DEFAULT now(),
>>>>>>> main
    PRIMARY KEY (address, block_number)
);

CREATE TABLE public.neo_account_slots (
    block_number BIGINT NOT NULL,
    slot_index BYTEA NOT NULL,
    account_address BYTEA NOT NULL,
    value BYTEA NOT NULL,
    created_at TIMESTAMP WITHOUT TIME ZONE DEFAULT now(),
    PRIMARY KEY (account_address, slot_index, block_number)
);

CREATE TABLE public.neo_transactions (
    block_number BIGINT NOT NULL,
    hash BYTEA NOT NULL,
    transaction_data JSONB NOT NULL,
    created_at TIMESTAMP WITHOUT TIME ZONE DEFAULT now(),
    PRIMARY KEY (hash)
);

CREATE TABLE public.neo_logs (
    block_number BIGINT NOT NULL,
    hash BYTEA NOT NULL,
    address BYTEA NOT NULL,
    log_idx numeric NOT NULL,
    log_data JSONB NOT NULL,
    created_at TIMESTAMP WITHOUT TIME ZONE DEFAULT now(),
    PRIMARY KEY (hash, block_number, log_idx)
);

--
-- PostgreSQL database dump complete
--

SET search_path TO "$user", public;

<<<<<<< HEAD
CREATE TABLE IF NOT EXISTS topics (
    topic BYTEA NOT NULL CHECK (LENGTH(topic) = 32)
    ,transaction_hash BYTEA REFERENCES transactions(hash) NOT NULL CHECK (LENGTH(transaction_hash) = 32)
    ,transaction_idx NUMERIC NOT NULL CHECK (transaction_idx >= 0)
    ,log_idx NUMERIC NOT NULL CHECK (log_idx >= 0)
    ,topic_idx NUMERIC NOT NULL CHECK (topic_idx >= 0)
    ,block_number NUMERIC REFERENCES blocks(number) ON DELETE CASCADE NOT NULL CHECK (block_number >= 0)
    ,block_hash BYTEA REFERENCES blocks(hash) NOT NULL CHECK (LENGTH(block_hash) = 32)
    ,FOREIGN KEY (block_hash, log_idx) REFERENCES logs (block_hash, log_idx)
    ,PRIMARY KEY (block_hash, log_idx, topic_idx)
);


-- Insert genesis block and the pre-funded account on database creation. These should be
-- present in the database regardless of how stratus is started.

INSERT INTO blocks(
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
VALUES (
    0, -- number
    decode('011b4d03dd8c01f1049143cf9c4c817e4b167f1d1b83e5c6f0f10d89ba1e7bce', 'hex'), -- hash
    decode('56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421', 'hex'), -- transactions_root
    4294967295, -- gas_limit
    0, -- gas_used
    decode('00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000', 'hex'), -- logs_bloom
    0, -- timestamp_in_secs
    decode('0000000000000000000000000000000000000000000000000000000000000000', 'hex'), -- parent_hash
    decode('0000000000000000000000000000000000000000', 'hex'), -- author
    decode('', 'hex'), -- extra_data
    decode('0000000000000000000000000000000000000000', 'hex'), -- miner
    0, -- difficulty
    decode('56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421', 'hex'), -- receipts_root
    decode('1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347', 'hex'), -- uncle_hash
    505, -- size
    decode('c7cc35e75df400dd94a7f2a4db18729e87dacab31eacc7ab4a26b41fc5e32937', 'hex'), -- state_root
    0, -- total_difficulty
    decode('0000000000000000', 'hex'), -- nonce
    current_timestamp -- created_at
);

INSERT INTO accounts (address, bytecode, code_hash, latest_balance, latest_nonce, creation_block)
VALUES (
    decode('F56A88A4afF45cdb5ED7Fe63a8b71aEAaFF24FA6', 'hex'),
    NULL,
    decode('c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470', 'hex'), -- keccak256 of empty string
    POWER(2, 256) - 1, -- U256 max balance
    0,
    0
);

INSERT INTO  historical_balances (address, balance, block_number)
VALUES (decode('F56A88A4afF45cdb5ED7Fe63a8b71aEAaFF24FA6', 'hex'), POWER(2, 256) - 1, 0);

INSERT INTO  historical_nonces (address, nonce, block_number)
VALUES (decode('F56A88A4afF45cdb5ED7Fe63a8b71aEAaFF24FA6', 'hex'), 0, 0);
=======
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
>>>>>>> main
