create table relayer_blocks(
    number bigint primary key not null check (number >= 0),
    payload jsonb not null,
    relayed boolean default false,
    success boolean default false
);

create table mismatches(
    id serial primary key not null,
    hash text not null,
    stratus_receipt jsonb not null,
    substrate_receipt jsonb not null,
    error text not null
);
