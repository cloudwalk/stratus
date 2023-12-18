CREATE TABLE IF NOT EXISTS accounts (
    id NUMERIC PRIMARY KEY,
    address BYTEA CHECK (LENGTH(address) = 40),
    nonce NUMERIC CHECK (nonce > 0),
    balance NUMERIC CHECK (balance > 0),
    bytecode BYTEA CHECK (LENGTH(bytecode) <= 24000)
);

CREATE TABLE IF NOT EXISTS account_slots (
    index NUMERIC PRIMARY KEY,
    value BYTEA CHECK (LENGTH(value) <= 32),
    account_id NUMERIC REFERENCES accounts(id)
);