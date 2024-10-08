use crate::metrics;

// JSON-RPC metrics.
metrics! {
    group: json_rpc,

    "Number of JSON-RPC requests active right now."
    gauge rpc_requests_active{},

    "Number of JSON-RPC requests that started."
    counter rpc_requests_started{client, method, contract, function},

    "Number of JSON-RPC requests that finished."
    histogram_duration rpc_requests_finished{client, method, contract, function, result, result_code, success},

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
    histogram_duration storage_read_account{storage, point_in_time, success},

    "Time executing storage read_block operation."
    histogram_duration storage_read_block{storage, success},

    "Time executing storage read_logs operation."
    histogram_duration storage_read_logs{storage, success},

    "Time executing storage read_slot operation."
    histogram_duration storage_read_slot{storage, point_in_time, success},

    "Time executing storage read_transaction operation."
    histogram_duration storage_read_transaction{storage, success}
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
    histogram_duration storage_finish_pending_block{storage, success},

    "Time executing storage save_block operation."
    histogram_duration storage_save_block{storage, size_by_tx, size_by_gas, success},

    "Time executing storage reset operation."
    histogram_duration storage_reset{storage, success}
}

// Importer online metrics.
metrics! {
    group: importer_online,

    "Time to import one block."
    histogram_duration import_online_mined_block{},

    "Number of transactions imported."
    counter importer_online_transactions_total{}
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

    "Count types of errors when executing a transaction."
    counter executor_transaction_error_types{error_type, client}
}

metrics! {
    group: evm,

    "Time executing EVM execution."
    histogram_duration evm_execution{point_in_time, success},

    "Number of accounts read in a single EVM execution."
    histogram_counter evm_execution_account_reads{},

    "Number of slots read in a single EVM execution."
    histogram_counter evm_execution_slot_reads{}
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
    gauge rocks_background_errors{dbname}
}

metrics! {
    group: consensus,

    "Time to run Consensus::forward."
    histogram_duration consensus_forward{},

    "The readiness of Stratus."
    gauge consensus_is_ready{}
}
