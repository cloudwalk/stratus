use crate::metrics;

// JSON-RPC metrics.
metrics! {
    group: json_rpc,

    "Number of JSON-RPC requests active right now."
    gauge rpc_requests_active{client, method},

    "Number of JSON-RPC requests that started."
    counter rpc_requests_started{client, method, function},

    "Number of JSON-RPC requests that finished."
    histogram_duration rpc_requests_finished{client, method, function, result, success},

    "Number of JSON-RPC subscriptions active right now."
    gauge rpc_subscriptions_active{subscription}
}

// Storage reads.
metrics! {
    group: storage_read,

    "Time to execute storage check_conflicts operation."
    histogram_duration storage_check_conflicts{success, conflicted},

    "Time to execute storage read_active_block_number operation."
    histogram_duration storage_read_active_block_number{success},

    "Time to execute storage read_mined_block_number operation."
    histogram_duration storage_read_mined_block_number{success},

    "Time to execute storage read_account operation."
    histogram_duration storage_read_account{found_at, point_in_time, success},

    "Time to execute storage read_block operation."
    histogram_duration storage_read_block{success},

    "Time to execute storage read_logs operation."
    histogram_duration storage_read_logs{success},

    "Time to execute storage read_slot operation."
    histogram_duration storage_read_slot{found_at, point_in_time, success},

    "Time to execute storage read_mined_transaction operation."
    histogram_duration storage_read_mined_transaction{success}
}

// Storage writes.
metrics! {
    group: storage_write,

    "Time to execute storage set_active_block_number operation."
    histogram_duration storage_set_active_block_number{success},

    "Time to execute storage set_mined_block_number operation."
    histogram_duration storage_set_mined_block_number{success},

    "Time to execute storage save_accounts operation."
    histogram_duration storage_save_accounts{success},

    "Time to execute storage save_account_changes operation."
    histogram_duration storage_save_execution{success},

    "Time to execute storage flush operation."
    histogram_duration storage_flush{kind, success},

    "Time to execute storage set_active_external_block operation."
    histogram_duration storage_set_active_external_block{success},

    "Time to execute storage finish_block operation."
    histogram_duration storage_finish_block{success},

    "Time to execute storage save_block operation."
    histogram_duration storage_save_block{size_by_tx, size_by_gas, success},

    "Time to execute storage reset operation."
    histogram_duration storage_reset{kind, success}
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

    "Time to execute and persist an external block with all transactions."
    histogram_duration executor_external_block{},

    "Time to execute and persist temporary changes of a single transaction inside import_offline operation."
    histogram_duration executor_external_transaction{function},

    "Gas spent to execute a single transaction inside import_offline operation."
    histogram_counter executor_external_transaction_gas{function},

    "Number of account reads when importing an external block."
    histogram_counter executor_external_block_account_reads{},

    "Number of slot reads when importing an external block."
    histogram_counter executor_external_block_slot_reads{},

    "Time to execute a transaction received with eth_sendRawTransaction."
    histogram_duration executor_transact{success, function},

    "Gas spent execute a transaction received with eth_sendRawTransaction."
    histogram_counter executor_transact_gas{success, function},

    "Time to execute a transaction received with eth_call or eth_estimateGas."
    histogram_duration executor_call{success, function},

    "Gas spent to execute a transaction received with eth_call or eth_estimateGas."
    histogram_counter executor_call_gas{function}
}

metrics! {
    group: evm,

    "Time to execute EVM execution."
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
    gauge rocks_bytes_read{dbname}
}

metrics! {
    group: consensus,

    "Time to run Consensus::append_block_to_peer."
    histogram_duration consensus_append_block_to_peer{},

    "Time to run Consensus::start_election."
    histogram_duration consensus_start_election{},

    "Time to run Consensus::forward."
    histogram_duration consensus_forward{},

    "The diff between what is on the follower database and what it received from Append Entries."
    gauge append_entries_block_number_diff{}
}

metrics! {
    group: external_relayer,

    "Time to run ExternalRelayer::relay_next_block."
    histogram_duration relay_next_block{},

    "Time to run ExternalRelayer::compute_tx_dag."
    histogram_duration compute_tx_dag{},

    "Time to run ExternalRelayer::relay_and_check_mempool."
    histogram_duration relay_and_check_mempool{},

    "Time to run ExternalRelayer::take_roots."
    histogram_duration take_roots{},

    "Time to run ExternalRelayer::relay_dag."
    histogram_duration relay_dag{},

    "Time to run ExternalRelayer::compare_receipts."
    histogram_duration compare_receipts{},

    "Number of execution mismatches."
    histogram_duration save_mismatch{},

    "Time to run ExternalRelayerClient::send_to_relayer."
    histogram_duration send_to_relayer{}
}
