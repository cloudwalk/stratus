use crate::metrics;

// JSON-RPC metrics.
metrics! {
    group: json_rpc,

    "Number of JSON-RPC requests that started."
    counter   rpc_requests_started{method, function} [],

    "Number of JSON-RPC requests that finished."
    histogram_duration rpc_requests_finished{method, function, success} []
}

// Storage reads.
metrics! {
    group: storage_read,

    "Time to execute storage read_active_block_number operation."
    histogram_duration storage_read_active_block_number{success} [],

    "Time to execute storage read_mined_block_number operation."
    histogram_duration storage_read_mined_block_number{success} [],

    "Time to execute storage read_account operation."
    histogram_duration storage_read_account{found_at, point_in_time, success} [],

    "Time to execute storage read_block operation."
    histogram_duration storage_read_block{success} [],

    "Time to execute storage read_logs operation."
    histogram_duration storage_read_logs{success} [],

    "Time to execute storage read_slot operation."
    histogram_duration storage_read_slot{found_at, point_in_time, success} [],

    "Time to execute storage read_slot operation."
    histogram_duration storage_read_slots{point_in_time, success} [],

    "Time to execute storage read_mined_transaction operation."
    histogram_duration storage_read_mined_transaction{success} []
}

// Storage writes.
metrics! {
    group: storage_write,

    "Time to execute storage set_active_block_number operation."
    histogram_duration storage_set_active_block_number{success} [],

    "Time to execute storage set_mined_block_number operation."
    histogram_duration storage_set_mined_block_number{success} [],

    "Time to execute storage save_accounts operation."
    histogram_duration storage_save_accounts{success} [],

    "Time to execute storage save_account_changes operation."
    histogram_duration storage_save_execution{success} [],

    "Time to execute storage flush_temp operation."
    histogram_duration storage_flush_temp{success} [],

    "Time to execute storage save_block operation."
    histogram_duration storage_save_block{success} [],

    "Time to execute storage reset operation."
    histogram_duration storage_reset{kind, success} [],

    "Time to execute storage commit operation."
    histogram_duration storage_commit{size_by_tx, size_by_gas, success} [],

    "Ammount of gas in the commited transactions"
    counter   storage_gas_total{} []
}

// Importer offline metrics.
metrics! {
    group: importer_offline,

    "Time to execute import_offline operation."
    histogram_duration import_offline{} []
}

// Importer online metrics.
metrics! {
    group: importer_online,

    "Time to execute import_online operation."
    histogram_duration import_online{} [],

    "Time to import one mined block."
    histogram_duration import_online_mined_block{} [],

    "Transactions imported"
    counter importer_online_transactions_total{} []
}

// Execution metrics.
metrics! {
    group: executor,

    "Time to execute and persist an external block with all transactions."
    histogram_duration executor_external_block{} [],

    "Time to execute and persist temporary changes of a single transaction inside import_offline operation."
    histogram_duration executor_external_transaction{function} [],

    "Gas spent to execute a single transaction inside import_offline operation."
    histogram_counter executor_external_transaction_gas{function} [],

    "Number of account reads when importing an external block."
    histogram_counter executor_external_block_account_reads{} [0., 1., 2., 3., 4., 5., 6., 7., 8., 9., 10., 20., 30., 40., 50., 60., 70., 80., 90., 100., 150., 200.],

    "Number of slot reads when importing an external block."
    histogram_counter executor_external_block_slot_reads{} [0., 10., 20., 30., 40., 50., 60., 70., 80., 90., 100., 200., 300., 400., 500., 600., 700., 800., 900., 1000., 2000., 3000., 4000., 5000., 6000., 7000., 8000., 9000., 10000.],

    "Number of slot reads cached when importing an external block."
    histogram_counter executor_external_block_slot_reads_cached{} [0., 10., 20., 30., 40., 50., 60., 70., 80., 90., 100., 200., 300., 400., 500., 600., 700., 800., 900., 1000., 2000., 3000., 4000., 5000., 6000., 7000., 8000., 9000., 10000.],

    "Time to execute a transaction received with eth_sendRawTransaction."
    histogram_duration executor_transact{success, function} [],

    "Gas spent execute a transaction received with eth_sendRawTransaction."
    histogram_counter executor_transact_gas{success, function} [],

    "Time to execute a transaction received with eth_call or eth_estimateGas."
    histogram_duration executor_call{success, function} [],

    "Gas spent to execute a transaction received with eth_call or eth_estimateGas."
    histogram_counter executor_call_gas{function} []
}

metrics! {
    group: evm,

    "Time to execute EVM execution."
    histogram_duration evm_execution{point_in_time, success} [],

    "Number of accounts read in a single EVM execution."
    histogram_counter evm_execution_account_reads{} [0., 1., 2., 3., 4., 5., 6., 7., 8., 9., 10.],

    "Number of slots read in a single EVM execution."
    histogram_counter evm_execution_slot_reads{} [0., 10., 20., 30., 40., 50., 60., 70., 80., 90., 100., 200., 300., 400., 500., 600., 700., 800., 900., 1000.],

    "Number of slots read cached in a single EVM execution."
    histogram_counter evm_execution_slot_reads_cached{} [0., 10., 20., 30., 40., 50., 60., 70., 80., 90., 100., 200., 300., 400., 500., 600., 700., 800., 900., 1000.]
}

metrics! {
    group: rocks,

    "Number of issued gets to rocksdb."
    gauge rocks_db_get{dbname} [],

    "Number of writes issued to rocksdb."
    gauge rocks_db_write{dbname} [],

    "Time spent compacting data."
    gauge rocks_compaction_time{dbname} [],

    "CPU time spent compacting data."
    gauge rocks_compaction_cpu_time{dbname} [],

    "Time spent flushing memtable to disk."
    gauge rocks_flush_time{dbname} [],

    "Number of block cache misses."
    gauge rocks_block_cache_miss{dbname} [],

    "Number of block cache hits."
    gauge rocks_block_cache_hit{dbname} [],

    "Number of bytes written."
    gauge rocks_bytes_written{dbname} [],

    "Number of bytes read."
    gauge rocks_bytes_read{dbname} []
}
