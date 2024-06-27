create table relayer_blocks(
    number bigint primary key not null check (number >= 0),
    payload jsonb not null,
    started boolean not null default false,
    finished boolean not null default false,
    mismatched boolean not null default false
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
    primary key (block_number, address, index)
);
create table tx_hash_map(
    stratus_hash bytea primary key not null,
    substrate_hash bytea not null,
    resigned_transaction jsonb not null
);
