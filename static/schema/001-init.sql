-- TODO: maybe call this table `block_headers`
CREATE TABLE IF NOT EXISTS blocks (
    number BIGSERIAL NOT NULL CHECK (number >= 0) UNIQUE
    ,hash BYTEA NOT NULL CHECK (LENGTH(hash) = 32) UNIQUE
    ,transactions_root BYTEA NOT NULL CHECK (LENGTH(transactions_root) = 32)
    ,gas NUMERIC NOT NULL CHECK (gas >= 0)
    ,logs_bloom BYTEA NOT NULL CHECK (LENGTH(logs_bloom) = 256)
    ,timestamp_in_secs BIGINT NOT NULL CHECK (timestamp_in_secs >= 0)
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

INSERT INTO blocks(number, hash, transactions_root, gas, logs_bloom, timestamp_in_secs, parent_hash, created_at)
VALUES (
    0,
    decode('066323b52e6ae4f5e99fefa32df446b9882ceb11734bc6f4d57d6e3a8f66b3a7', 'hex'),
    decode('56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421', 'hex'),
    0,
    decode('00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000', 'hex'),
    0,
    decode('0000000000000000000000000000000000000000000000000000000000000000', 'hex'),
    current_timestamp
);

INSERT INTO accounts (address, bytecode, latest_balance, latest_nonce, creation_block)
VALUES (
    decode('F56A88A4afF45cdb5ED7Fe63a8b71aEAaFF24FA6', 'hex'),
    NULL,
    POWER(2, 256) - 1,
    0,
    0
);

INSERT INTO  historical_balances (address, balance, block_number)
VALUES (decode('F56A88A4afF45cdb5ED7Fe63a8b71aEAaFF24FA6', 'hex'), POWER(2, 256) - 1, 0);

INSERT INTO  historical_nonces (address, nonce, block_number)
VALUES (decode('F56A88A4afF45cdb5ED7Fe63a8b71aEAaFF24FA6', 'hex'), 0, 0);
