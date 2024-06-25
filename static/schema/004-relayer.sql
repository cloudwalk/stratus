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
create table slot_mismatches(
    address bytea not null,
    index bytea not null,
    block_number bigint not null,
    stratus_value bytea not null,
    substrate_value bytea not null,
    primary key (address, index)
);
create table tx_hash_map(
    stratus_hash bytea primary key not null,
    substrate_hash bytea not null
);
