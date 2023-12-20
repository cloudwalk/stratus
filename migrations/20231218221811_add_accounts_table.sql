CREATE TABLE IF NOT EXISTS accounts (
    address BYTEA NOT NULL CHECK (LENGTH(address) = 20),
    nonce NUMERIC NOT NULL CHECK (nonce >= 0),
    balance NUMERIC NOT NULL CHECK (balance >= 0),
    bytecode BYTEA CHECK (LENGTH(bytecode) <= 24000),
    PRIMARY KEY (address)
);

CREATE TABLE IF NOT EXISTS account_slots (
    idx BYTEA NOT NULL CHECK (LENGTH(idx) = 32),
    value BYTEA NOT NULL CHECK (LENGTH(value) = 32),
    account_address BYTEA NOT NULL REFERENCES accounts (address),
    PRIMARY KEY (idx, account_address)
);

CREATE TABLE IF NOT EXISTS blocks (
    number SERIAL NOT NULL CHECK (number >= 0),
    hash BYTEA NOT NULL CHECK (LENGTH(hash) = 32),
    transactions_root BYTEA NOT NULL CHECK (LENGTH(transactions_root) = 32),
    created_at TIMESTAMP NOT NULL
    -- PRIMARY KEY?
);

CREATE TABLE IF NOT EXISTS transactions (
    hash BYTEA NOT NULL CHECK (LENGTH(hash) = 32),
    signer_address BYTEA NOT NULL CHECK (LENGTH(signer_address) = 20),
    idx_in_block INT NOT NULL CHECK (idx_in_block >= 0),
    block_number SERIAL NOT NULL CHECK (number >= 0),
    block_hash BYTEA NOT NULL CHECK (LENGTH(block_hash) = 32)
    -- PRIMARY KEY?
);

CREATE SEQUENCE IF NOT EXISTS block_number_seq
AS BIGINT
MINVALUE 0
START WITH 0
INCREMENT BY 1
NO CYCLE
OWNED BY blocks.number;
