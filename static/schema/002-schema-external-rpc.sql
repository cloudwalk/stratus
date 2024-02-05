create table external_blocks(
    number bigint primary key not null check (number >= 0),
    payload jsonb
);

create table external_receipts(
    hash bytea primary key not null check (length(hash) = 32),
    payload jsonb
);