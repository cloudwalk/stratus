use crate::metrics;

// JSON-RPC metrics.
metrics! {
    group: json_rpc,

    "Number of JSON-RPC requests active right now."
    gauge rpc_requests_active{},

    "Number of JSON-RPC requests that started."
    counter rpc_requests_started{client, method, contract, function, req_type},

    "Number of JSON-RPC requests that finished."
    histogram_duration rpc_requests_finished{client, method, contract, function, result, result_code, success},

    "Response size in bytes for JSON-RPC responses."
    histogram_counter rpc_response_size{client, method},

    "Number of JSON-RPC subscriptions active right now."
    gauge rpc_subscriptions_active{subscription, client}
}

// Storage reads.
metrics! {
    group: storage_read,

    "Time executing storage read_pending_block_number operation."
    histogram_duration storage_read_pending_block_number{storage, success},

    "Time executing storage read_mined_block_number operation."
    histogram_duration storage_read_mined_block_number{storage, success},

    "Time executing storage read_account operation."
    histogram_duration storage_read_account{storage, point_in_time, hit},

    "Time executing storage read_block operation."
    histogram_duration storage_read_block{storage, success},

    "Time executing storage read_block_with_changes operation."
    histogram_duration storage_read_block_with_changes{storage, success},

    "Time executing storage read_logs operation."
    histogram_duration storage_read_logs{storage, success},

    "Time executing storage read_slot operation."
    histogram_duration storage_read_slot{storage, point_in_time, hit},

    "Time executing storage read_transaction operation."
    histogram_duration storage_read_transaction{storage, success},

    "Time executing storage read_replication_log operation."
    histogram_duration storage_read_replication_log{storage, success}
}

// Storage writes.
metrics! {
    group: storage_write,

    "Time executing storage set_pending_block_number operation."
    histogram_duration storage_set_pending_block_number{storage, success},

    "Time executing storage set_mined_block_number operation."
    histogram_duration storage_set_mined_block_number{storage, success},

    "Time executing storage save_accounts operation."
    histogram_duration storage_save_accounts{storage, success},

    "Time executing storage save_account_changes operation."
    histogram_duration storage_save_execution{storage, success},

    "Time executing storage set_pending_external_block operation."
    histogram_duration storage_set_pending_external_block{storage, success},

    "Time executing storage finish_pending_block operation."
    histogram_duration storage_finish_pending_block{},

    "Time executing storage save_block operation."
    histogram_duration storage_save_block{storage, tens_of_millions_gas_used, success},

    "Time executing storage reset operation."
    histogram_duration storage_reset{storage, success},

    "Time executing storage apply_replication_log operation."
    histogram_duration storage_apply_replication_log{storage, success}
}

// Importer online metrics.
metrics! {
    group: importer_online,

    "Time to import one block."
    histogram_duration import_online_mined_block{},

    "Time to fetch one block."
    histogram_duration import_online_fetched_block{},

    "Time to post-process one fetched block."
    histogram_duration import_online_post_process_block{},

    "Total time to fetch and post-process one block."
    histogram_duration import_online_fetch_and_post_process_block{},

    "Number of transactions imported."
    counter importer_online_transactions_total{},

    "Number of blocks between follower and leader, determined by the direction label (Ahead or Behind)."
    gauge importer_online_lag_blocks{direction}
}

// Execution metrics.
metrics! {
    group: executor,

    "Time executing and persist an external block with all transactions."
    histogram_duration executor_external_block{},

    "Time executing an external transaction."
    histogram_duration executor_external_transaction{contract, function},

    "Number of account reads executing an external transaction."
    histogram_counter executor_external_transaction_account_reads{contract, function},

    "Number of slot reads executing an external transaction."
    histogram_counter executor_external_transaction_slot_reads{contract, function},

    "Gas spent executing an external transaction."
    histogram_counter executor_external_transaction_gas{contract, function},

    "Number of account reads when importing an external block."
    histogram_counter executor_external_block_account_reads{},

    "Number of slot reads when importing an external block."
    histogram_counter executor_external_block_slot_reads{},

    "Time executing a local transaction."
    histogram_duration executor_local_transaction{success, contract, function},

    "Time waiting to acquire the local transaction execution lock."
    histogram_duration executor_local_transaction_lock_wait{},

    "Time executing a local transaction."
    counter executor_local_transaction_reverts{contract, function, reason},

    "Number of account reads when executing a local transaction."
    histogram_counter executor_local_transaction_account_reads{contract, function},

    "Number of slot reads when executing a local transaction."
    histogram_counter executor_local_transaction_slot_reads{contract, function},

    "Gas spent executing a local transaction."
    histogram_counter executor_local_transaction_gas{success, contract, function},

    "Time executing a transaction received with eth_call or eth_estimateGas."
    histogram_duration executor_local_call{success, contract, function},

    "Number of account reads when executing a local call."
    histogram_counter executor_local_call_account_reads{contract, function},

    "Number of slot reads when executing a local call."
    histogram_counter executor_local_call_slot_reads{contract, function},

    "Gas spent executing a local call."
    histogram_counter executor_local_call_gas{contract, function},

    "Time executing trace_transaction"
    histogram_duration executor_inspect{trace_type},

    "Number of EVM pool workers busy executing right now."
    gauge executor_workers_busy{pool}
}

// revmc JIT/AOT runtime statistics.
//
// revmc only exposes pull-based counters, so these are sampled every few seconds by a
// dedicated reporter thread (see `spawn_metrics_reporter`) and exported as gauges.
metrics! {
    group: executor_jit,

    "Dispatches that found compiled code. Counted once per code hash per EVM (decisions are cached and periodically refreshed), not once per execution."
    gauge executor_jit_lookup_hits{mode},

    "Dispatches with no compiled code ready yet (warmup or failed compilation). Counted once per code hash per EVM, not once per execution."
    gauge executor_jit_lookup_misses{mode},

    "Total revmc compilations dispatched after contracts became hot."
    gauge executor_jit_compilations_dispatched{mode},

    "Total revmc compilations that succeeded."
    gauge executor_jit_compilations_succeeded{mode},

    "Total revmc compilations that failed."
    gauge executor_jit_compilations_failed{mode},

    "revmc compilation jobs currently in flight."
    gauge executor_jit_compilations_pending{mode},

    "Compiled contracts currently resident in the JIT code cache."
    gauge executor_jit_resident_entries{mode},

    "Compiled entries evicted from the JIT code cache (idle or budget)."
    gauge executor_jit_evictions{mode},

    "Hot-counter events dropped because the revmc event queue was full."
    gauge executor_jit_events_dropped{mode},

    "Bytes allocated by LLVM for executable JIT code sections."
    gauge executor_jit_code_bytes{mode}
}

metrics! {
    group: rocks,

    "Number of issued gets to rocksdb."
    gauge rocks_db_get{dbname},

    "Number of writes issued to rocksdb."
    gauge rocks_db_write{dbname},

    "Time spent compacting data."
    gauge rocks_compaction_time{dbname},

    "CPU time spent compacting data."
    gauge rocks_compaction_cpu_time{dbname},

    "Time spent flushing memtable to disk."
    gauge rocks_flush_time{dbname},

    "Number of block cache misses."
    gauge rocks_block_cache_miss{dbname},

    "Number of block cache hits."
    gauge rocks_block_cache_hit{dbname},

    "Number of bytes written."
    gauge rocks_bytes_written{dbname},

    "Number of bytes read."
    gauge rocks_bytes_read{dbname},

    "Number of times WAL sync is done."
    gauge rocks_wal_file_synced{dbname},

    "Last startup delay."
    gauge rocks_last_startup_delay_millis{dbname},

    "Last shutdown delay."
    gauge rocks_last_shutdown_delay_millis{dbname},

    "Approximate size of active memtable (bytes)."
    gauge rocks_cur_size_active_mem_table{dbname},

    "Approximate size of active and unflushed immutable memtables (bytes)."
    gauge rocks_cur_size_all_mem_tables{dbname},

    "Approximate of active, unflushed immutable, and pinned immutable memtables (bytes)."
    gauge rocks_size_all_mem_tables{dbname},

    "Memory size for the entries residing in block cache."
    gauge rocks_block_cache_usage{dbname},

    "Block cache capacity."
    gauge rocks_block_cache_capacity{dbname},

    "Accumulated number of background errors."
    gauge rocks_background_errors{dbname},

    "Size of column family on disk (bytes)."
    gauge rocks_cf_size{dbname, cfname}
}

metrics! {
    group: consensus,

    "Time to run Consensus::forward."
    histogram_duration consensus_forward{},

    "The readiness of Stratus."
    gauge consensus_is_ready{}
}

// Kafka Metrics
metrics! {
    group: kafka,

    "Time to run KafkaConnector::queue_event"
    histogram_duration kafka_queue_event{},

    "Time to run KafkaConnector::send_buffered"
    histogram_duration kafka_send_buffered{},

    "Time to run KafkaConnector::create_buffer"
    histogram_duration kafka_create_buffer{}
}
