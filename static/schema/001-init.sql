-- TODO: maybe call this table `block_headers`
CREATE TABLE IF NOT EXISTS blocks (
    number NUMERIC NOT NULL CHECK (number >= 0) UNIQUE,
    hash BYTEA NOT NULL CHECK (LENGTH(hash) = 32) UNIQUE,
    transactions_root BYTEA NOT NULL CHECK (LENGTH(transactions_root) = 32),
    gas_limit NUMERIC NOT NULL CHECK (gas_limit >= 0),
    gas_used NUMERIC NOT NULL CHECK (gas_used >= 0),
    logs_bloom BYTEA NOT NULL CHECK (LENGTH(logs_bloom) = 256),
    timestamp_in_secs NUMERIC NOT NULL CHECK (timestamp_in_secs >= 0),
    parent_hash BYTEA NOT NULL CHECK (LENGTH(parent_hash) = 32) UNIQUE,
    author BYTEA NOT NULL CHECK (LENGTH(author) = 20),
    extra_data BYTEA NOT NULL CHECK (LENGTH(extra_data) <= 32),
    miner BYTEA NOT NULL CHECK (LENGTH(miner) = 20),
    difficulty NUMERIC NOT NULL CHECK (difficulty >= 0),
    receipts_root BYTEA NOT NULL CHECK (LENGTH(receipts_root) = 32),
    uncle_hash BYTEA NOT NULL CHECK (LENGTH(uncle_hash) = 32),
    size NUMERIC NOT NULL CHECK (size >= 0),
    state_root BYTEA NOT NULL CHECK (LENGTH(state_root) = 32),
    total_difficulty NUMERIC NOT NULL CHECK (total_difficulty >= 0),
    nonce BYTEA NOT NULL CHECK (LENGTH(nonce) = 8),
    created_at TIMESTAMP NOT NULL,
    PRIMARY KEY (number, hash)
);

CREATE TABLE IF NOT EXISTS accounts (
    address BYTEA NOT NULL CHECK (LENGTH(address) = 20),
    bytecode BYTEA,
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
    PRIMARY KEY (address, block_number)
);

CREATE TABLE IF NOT EXISTS historical_nonces (
    address BYTEA NOT NULL CHECK (LENGTH(address) = 20) REFERENCES accounts (address) ON DELETE CASCADE,
    nonce NUMERIC NOT NULL CHECK (nonce >= 0),
    block_number NUMERIC NOT NULL REFERENCES blocks (number) ON DELETE CASCADE,
    PRIMARY KEY (address, block_number)
);

CREATE TABLE IF NOT EXISTS account_slots (
    idx BYTEA NOT NULL CHECK (LENGTH(idx) = 32),
    value BYTEA NOT NULL CHECK (LENGTH(value) = 32),
    account_address BYTEA NOT NULL REFERENCES accounts (address) ON DELETE CASCADE,
    previous_value BYTEA CHECK (LENGTH(value) = 32),
    creation_block NUMERIC NOT NULL REFERENCES blocks (number) ON DELETE CASCADE,
    PRIMARY KEY (idx, account_address)
);

CREATE TABLE IF NOT EXISTS historical_slots (
    idx BYTEA NOT NULL CHECK (LENGTH(idx) = 32),
    value BYTEA NOT NULL CHECK (LENGTH(value) = 32),
    account_address BYTEA NOT NULL REFERENCES accounts (address) ON DELETE CASCADE,
    block_number NUMERIC NOT NULL CHECK (block_number >= 0) REFERENCES blocks (number) ON DELETE CASCADE,
    PRIMARY KEY (idx, account_address, block_number)
);

CREATE TABLE IF NOT EXISTS transactions (
    hash BYTEA NOT NULL CHECK (LENGTH(hash) = 32) UNIQUE,
    signer_address BYTEA NOT NULL CHECK (LENGTH(signer_address) = 20),
    nonce NUMERIC NOT NULL CHECK (nonce >= 0),
    address_from BYTEA NOT NULL CHECK (LENGTH(address_from) = 20),
    address_to BYTEA CHECK  (LENGTH(address_to) = 20),
    input BYTEA NOT NULL
    ,output BYTEA NOT NULL
    ,gas NUMERIC NOT NULL CHECK (gas >= 0)
    ,gas_price NUMERIC NOT NULL CHECK (gas_price >= 0)
    ,idx_in_block NUMERIC NOT NULL CHECK (idx_in_block >= 0)
    ,block_number NUMERIC REFERENCES blocks(number) ON DELETE CASCADE NOT NULL CHECK (block_number >= 0)
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
    ,transaction_idx NUMERIC NOT NULL CHECK (transaction_idx >= 0)
    ,log_idx NUMERIC NOT NULL CHECK (log_idx >= 0)
    ,block_number NUMERIC REFERENCES blocks(number) ON DELETE CASCADE NOT NULL CHECK (block_number >= 0)
    ,block_hash BYTEA REFERENCES blocks(hash) NOT NULL CHECK (LENGTH(block_hash) = 32)
    ,PRIMARY KEY (block_hash, log_idx)
);

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
