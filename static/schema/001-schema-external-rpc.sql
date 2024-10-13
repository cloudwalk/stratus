create table external_blocks(
    number bigint primary key not null check (number >= 0),
    block jsonb not null,
    receipts jsonb[] not null
);

create table external_balances(
    address bytea primary key not null check (length(address) = 20),
    balance numeric not null check (balance >= 0)
);
