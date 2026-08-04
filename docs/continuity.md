# Block continuity

## Chain progress

Stratus tracks two in-memory chain tips:

- `latest_sealed` is the execution tip. Temporary storage advances it whenever a block is sealed and uses it to build the next block header.
- `last_saved` is the durable tip. It advances only after permanent storage successfully saves a block.

Both tips contain the block number and hash. On a populated database they are initialized from the latest permanent block. On an empty database, temporary storage starts from the canonical sealed genesis while `last_saved` remains empty until genesis is persisted. During normal leader and follower operation they usually advance together. The offline importer can seal blocks faster than it saves them, so `latest_sealed` may be far ahead of `last_saved`.

```mermaid
flowchart LR
    subgraph permanent["Permanent storage"]
        direction LR
        P0["Block N-1<br/>persisted"] --> P1["Block N<br/>last_saved"]
    end

    subgraph temporary["Temporary sealed chain"]
        direction LR
        T1["Block N+1<br/>sealed, unsaved"] --> T2["Block N+2<br/>sealed, unsaved"]
        T2 --> T3["Block N+3<br/>latest_sealed"]
    end

    P1 --> T1
```

Sealing uses `latest_sealed.hash` as the next header's `parent_hash`, then advances `latest_sealed` to the newly sealed block. This operation does not decide what belongs to the durable chain.

`save_block` is the universal continuity boundary for leader mining, follower reexecution, and follower replication. Before saving block `N`, it validates in memory that:

```text
N == last_saved.number + 1
N.parent_hash == last_saved.hash
```

Permanent storage is written only after those checks pass. `last_saved` advances to `N` only after the write succeeds. No permanent-storage read is required during saving.

If the process restarts, sealed-but-unsaved work is discarded. Both tips are restored from the durable permanent tip and the unsaved range is executed again.

## Block-hash cache

The block-hash cache is independent of chain progress. Neither sealing nor saving uses it to decide the parent or validate continuity.

```mermaid
flowchart LR
    EVM["EVM BLOCKHASH"] --> Cache["Block-hash cache"]
    Cache -->|hit| Result["Block hash"]
    Cache -->|miss| Permanent["Permanent storage"]
    Permanent --> Result

    Offline["Offline importer<br/>sealed, unsaved hashes"] -. "temporary workaround" .-> Cache
```

Its normal purpose is to accelerate the `BLOCKHASH` opcode, with permanent storage as the source on a cache miss.

The offline importer is a temporary exception: execution can run ahead of persistence, so hashes of sealed-but-unsaved blocks exist only in memory. The importer currently publishes those hashes into the cache so `BLOCKHASH` can resolve them before they are saved.

This importer dependency is a workaround, not part of the chain-continuity model. When the offline importer is removed, remove the workaround and reduce the block-hash cache to the size needed only for opcode performance.

## Temporary storage naming

`InmemoryTransactionTemporaryStorage` and its `transaction_storage` field are misleading names. The component does not represent one Ethereum transaction or a database transaction. It owns block-level execution state:

- The pending block header, all transaction executions, and their aggregated account and slot changes.
- The latest finished block state and its hash, used while building the next block.
- The transition that moves the pending block to latest and creates the next pending block.

For example, an interval-mined pending block can accumulate many Ethereum transactions in this storage. When that block is sealed, the entire pending state—not an individual transaction—becomes `latest_block`.

A future focused refactor should rename it to something that reflects this responsibility. Suitable options include:

- `InMemoryBlockStateStorage` with a `block_storage` field.
- `InMemoryExecutionStateStorage` with an `execution_storage` field.

`InMemoryBlockStateStorage` is preferred because pending/latest block ownership is the component's defining responsibility.
