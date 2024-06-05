create table relayer_blocks(
    number bigint primary key not null check (number >= 0),
    payload jsonb not null,
    started boolean default false,
    finished boolean default false,
    mismatched boolean default false
);

create table mismatches(
    hash bytea primary key not null,
    block_number bigint,
    stratus_receipt jsonb not null,
    substrate_receipt jsonb not null,
    error text not null
);
