create table external_blocks(
    number bigint primary key not null check (number >= 0),
    payload jsonb not null
);

create table external_receipts(
    hash bytea primary key not null check (length(hash) = 32),
    block_number bigint not null references external_blocks(number),
    payload jsonb not null
);

create table external_balances(
    address bytea primary key not null check (length(address) = 20),
    balance numeric not null check (balance >= 0)
);