-- TODO: maybe call this table `block_headers`
CREATE TABLE IF NOT EXISTS blocks (
    number BIGSERIAL NOT NULL CHECK (number >= 0) UNIQUE
    ,hash BYTEA NOT NULL CHECK (LENGTH(hash) = 32) UNIQUE
    ,transactions_root BYTEA NOT NULL CHECK (LENGTH(transactions_root) = 32)
    ,gas NUMERIC NOT NULL CHECK (gas >= 0)
    ,logs_bloom BYTEA NOT NULL CHECK (LENGTH(logs_bloom) = 256)
    ,timestamp_in_secs INTEGER NOT NULL CHECK (timestamp_in_secs >= 0) -- UNIQUE
    ,parent_hash BYTEA NOT NULL CHECK (LENGTH(parent_hash) = 32) UNIQUE
    ,created_at TIMESTAMP NOT NULL
    ,PRIMARY KEY (number, hash)
);

CREATE TABLE IF NOT EXISTS accounts (
    address BYTEA NOT NULL CHECK (LENGTH(address) = 20),
    bytecode BYTEA CHECK (LENGTH(bytecode) <= 24000),
    latest_balance NUMERIC NOT NULL CHECK (latest_balance >= 0),
    latest_nonce NUMERIC NOT NULL CHECK (latest_nonce >= 0),
    creation_block  BIGSERIAL NOT NULL REFERENCES blocks (number) ON DELETE CASCADE,
    PRIMARY KEY (address)
);

CREATE TABLE IF NOT EXISTS historical_balances (
    address BYTEA NOT NULL CHECK (LENGTH(address) = 20) REFERENCES accounts (address) ON DELETE CASCADE,
    balance NUMERIC NOT NULL CHECK (balance >= 0),
    block_number BIGSERIAL NOT NULL REFERENCES blocks (number) ON DELETE CASCADE,
    PRIMARY KEY (address, block_number)
);

CREATE TABLE IF NOT EXISTS historical_nonces (
    address BYTEA NOT NULL CHECK (LENGTH(address) = 20) REFERENCES accounts (address) ON DELETE CASCADE,
    nonce NUMERIC NOT NULL CHECK (nonce >= 0),
    block_number BIGSERIAL NOT NULL REFERENCES blocks (number) ON DELETE CASCADE,
    PRIMARY KEY (address, block_number)
);

CREATE TABLE IF NOT EXISTS account_slots (
    idx BYTEA NOT NULL CHECK (LENGTH(idx) = 32),
    value BYTEA NOT NULL CHECK (LENGTH(value) = 32),
    account_address BYTEA NOT NULL REFERENCES accounts (address) ON DELETE CASCADE,
    creation_block  BIGSERIAL NOT NULL REFERENCES blocks (number) ON DELETE CASCADE,
    PRIMARY KEY (idx, account_address)
);

CREATE TABLE IF NOT EXISTS historical_slots (
    idx BYTEA NOT NULL CHECK (LENGTH(idx) = 32),
    value BYTEA NOT NULL CHECK (LENGTH(value) = 32),
    account_address BYTEA NOT NULL REFERENCES accounts (address) ON DELETE CASCADE,
    block_number BIGSERIAL NOT NULL CHECK (block_number >= 0) REFERENCES blocks (number) ON DELETE CASCADE,
    PRIMARY KEY (idx, account_address, block_number)
);

CREATE TABLE IF NOT EXISTS transactions (
    hash BYTEA NOT NULL CHECK (LENGTH(hash) = 32) UNIQUE,
    signer_address BYTEA NOT NULL CHECK (LENGTH(signer_address) = 20),
    nonce NUMERIC NOT NULL CHECK (nonce >= 0),
    address_from BYTEA NOT NULL CHECK (LENGTH(address_from) = 20),
    address_to BYTEA CHECK  (LENGTH(address_to) = 20),
    input BYTEA NOT NULL CHECK (LENGTH(input) <= 24000)
    ,output BYTEA NOT NULL
    ,gas NUMERIC NOT NULL CHECK (gas >= 0)
    ,gas_price NUMERIC NOT NULL CHECK (gas_price >= 0)
    ,idx_in_block SERIAL NOT NULL CHECK (idx_in_block >= 0)
    ,block_number BIGSERIAL REFERENCES blocks(number) ON DELETE CASCADE NOT NULL CHECK (block_number >= 0)
    ,block_hash BYTEA REFERENCES blocks(hash) NOT NULL CHECK (LENGTH(block_hash) = 32)
    ,v BYTEA NOT NULL
    ,r BYTEA NOT NULL
    ,s BYTEA NOT NULL
    ,value NUMERIC NOT NULL CHECK (value >= 0)
    ,result TEXT NOT NULL
    ,PRIMARY KEY (hash)
);

CREATE TABLE IF NOT EXISTS logs (
    address BYTEA NOT NULL CHECK (LENGTH(address) = 20)
    ,data BYTEA NOT NULL
    ,transaction_hash BYTEA REFERENCES transactions(hash) NOT NULL CHECK (LENGTH(transaction_hash) = 32)
    ,transaction_idx SERIAL NOT NULL CHECK (transaction_idx >= 0)
    ,log_idx SERIAL NOT NULL CHECK (log_idx >= 0)
    ,block_number BIGSERIAL REFERENCES blocks(number) ON DELETE CASCADE NOT NULL CHECK (block_number >= 0)
    ,block_hash BYTEA REFERENCES blocks(hash) NOT NULL CHECK (LENGTH(block_hash) = 32)
    ,PRIMARY KEY (block_hash, log_idx)
);

CREATE TABLE IF NOT EXISTS topics (
    topic BYTEA NOT NULL CHECK (LENGTH(topic) = 32)
    ,transaction_hash BYTEA REFERENCES transactions(hash) NOT NULL CHECK (LENGTH(transaction_hash) = 32)
    ,transaction_idx SERIAL NOT NULL CHECK (transaction_idx >= 0)
    ,log_idx SERIAL NOT NULL CHECK (log_idx >= 0)
    ,topic_idx SERIAL NOT NULL CHECK (topic_idx >= 0)
    ,block_number BIGSERIAL REFERENCES blocks(number) ON DELETE CASCADE NOT NULL CHECK (block_number >= 0)
    ,block_hash BYTEA REFERENCES blocks(hash) NOT NULL CHECK (LENGTH(block_hash) = 32)
    ,FOREIGN KEY (block_hash, log_idx) REFERENCES logs (block_hash, log_idx)
    ,PRIMARY KEY (block_hash, log_idx, topic_idx)
);
