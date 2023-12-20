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
