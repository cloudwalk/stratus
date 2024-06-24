use rocksdb::BlockBasedOptions;
use rocksdb::Cache;
use rocksdb::Options;

#[derive(Debug, Clone, Copy)]
pub enum CacheSetting {
    /// Enabled cache with the given size in bytes
    Enabled(usize),
    Disabled,
}

#[derive(Debug, Clone, Copy)]
pub enum DbConfig {
    LargeSSTFiles,
    FastWriteSST,
    Default,
}

impl Default for DbConfig {
    fn default() -> Self {
        Self::Default
    }
}

impl DbConfig {
    pub fn to_options(self, cache_setting: CacheSetting) -> Options {
        let mut opts = Options::default();
        let mut block_based_options = BlockBasedOptions::default();

        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        opts.increase_parallelism(16);

        // NOTE: As per the rocks db wiki: "The overhead of statistics is usually small but non-negligible. We usually observe an overhead of 5%-10%."
        #[cfg(feature = "metrics")]
        {
            opts.enable_statistics();
            opts.set_statistics_level(rocksdb::statistics::StatsLevel::ExceptTimeForMutex);
        }

        match self {
            DbConfig::LargeSSTFiles => {
                // Set the compaction style to Level Compaction
                opts.set_compaction_style(rocksdb::DBCompactionStyle::Level);

                // Configure the size of SST files at each level
                opts.set_target_file_size_base(512 * 1024 * 1024);

                // Increase the file size multiplier to expand file size at upper levels
                opts.set_target_file_size_multiplier(2); // Each level grows in file size quicker

                // Reduce the number of L0 files that trigger compaction, increasing frequency
                opts.set_level_zero_file_num_compaction_trigger(2);

                // Reduce thresholds for slowing and stopping writes, which forces more frequent compaction
                opts.set_level_zero_slowdown_writes_trigger(10);
                opts.set_level_zero_stop_writes_trigger(20);

                // Increase the max bytes for L1 to allow more data before triggering compaction
                opts.set_max_bytes_for_level_base(2048 * 1024 * 1024);

                // Increase the level multiplier to aggressively increase space at each level
                opts.set_max_bytes_for_level_multiplier(8.0); // Exponential growth of levels is more pronounced

                // Configure block size to optimize for larger blocks, improving sequential read performance
                block_based_options.set_block_size(128 * 1024); // 128KB blocks

                // Increase the number of write buffers to delay flushing, optimizing CPU usage for compaction
                opts.set_max_write_buffer_number(5);
                opts.set_write_buffer_size(128 * 1024 * 1024); // 128MB per write buffer

                // Keep a higher number of open files to accommodate more files being produced by aggressive compaction
                opts.set_max_open_files(20000);

                // Apply more aggressive compression settings, if I/O and CPU permit
                opts.set_compression_per_level(&[
                    rocksdb::DBCompressionType::Lz4,
                    rocksdb::DBCompressionType::Zstd, // Use Zstd for higher compression from L1 onwards
                ]);
            }
            DbConfig::FastWriteSST => {
                // Continue using Level Compaction due to its effective use of I/O and CPU for writes
                opts.set_compaction_style(rocksdb::DBCompactionStyle::Level);

                // Increase initial SST file sizes to reduce the frequency of writes to disk
                opts.set_target_file_size_base(512 * 1024 * 1024); // Starting at 512MB for L1

                // Minimize the file size multiplier to control the growth of file sizes at upper levels
                opts.set_target_file_size_multiplier(1); // Minimal increase in file size at upper levels

                // Increase triggers for write slowdown and stop to maximize buffer before I/O actions
                opts.set_level_zero_file_num_compaction_trigger(100); // Slow down writes at 100 L0 files
                opts.set_level_zero_stop_writes_trigger(200); // Stop writes at 200 L0 files

                // Expand the maximum bytes for base level to further delay the need for compaction-related I/O
                opts.set_max_bytes_for_level_base(2048 * 1024 * 1024);

                // Use a higher level multiplier to increase space exponentially at higher levels
                opts.set_max_bytes_for_level_multiplier(10.0);

                // Opt for larger block sizes to decrease the number of read and write operations to disk
                block_based_options.set_block_size(512 * 1024); // 512KB blocks

                // Maximize the use of write buffers to extend the time data stays in memory before flushing
                opts.set_max_write_buffer_number(16);
                opts.set_write_buffer_size(1024 * 1024 * 1024); // 1GB per write buffer

                // Allow a very high number of open files to minimize the overhead of opening and closing files
                opts.set_max_open_files(20000);

                // Choose compression that balances CPU use and effective storage reduction
                opts.set_compression_per_level(&[rocksdb::DBCompressionType::Lz4, rocksdb::DBCompressionType::Zstd]);

                // Enable settings that make full use of CPU to handle more data in memory and process compaction
                opts.set_allow_concurrent_memtable_write(true);
                opts.set_enable_write_thread_adaptive_yield(true);
            }
            DbConfig::Default => {
                block_based_options.set_ribbon_filter(15.5); // https://github.com/facebook/rocksdb/wiki/RocksDB-Bloom-Filter

                opts.set_allow_concurrent_memtable_write(true);
                opts.set_enable_write_thread_adaptive_yield(true);

                let transform = rocksdb::SliceTransform::create_fixed_prefix(10);
                opts.set_prefix_extractor(transform);
                opts.set_memtable_prefix_bloom_ratio(0.2);

                // Enable a size-tiered compaction style, which is good for workloads with a high rate of updates and overwrites
                opts.set_compaction_style(rocksdb::DBCompactionStyle::Universal);

                let mut universal_compact_options = rocksdb::UniversalCompactOptions::default();
                universal_compact_options.set_size_ratio(10);
                universal_compact_options.set_min_merge_width(2);
                universal_compact_options.set_max_merge_width(6);
                universal_compact_options.set_max_size_amplification_percent(50);
                universal_compact_options.set_compression_size_percent(-1);
                universal_compact_options.set_stop_style(rocksdb::UniversalCompactionStopStyle::Total);
                opts.set_universal_compaction_options(&universal_compact_options);

                let pt_opts = rocksdb::PlainTableFactoryOptions {
                    user_key_length: 0,
                    bloom_bits_per_key: 10,
                    hash_table_ratio: 0.75,
                    index_sparseness: 8,
                    encoding_type: rocksdb::KeyEncodingType::Plain, // Default encoding
                    full_scan_mode: false,                          // Optimized for point lookups rather than full scans
                    huge_page_tlb_size: 0,                          // Not using huge pages
                    store_index_in_file: false,                     // Store index in memory for faster access
                };
                opts.set_plain_table_factory(&pt_opts);
            }
        }
        if let CacheSetting::Enabled(cache_size) = cache_setting {
            let cache = Cache::new_lru_cache(cache_size);
            block_based_options.set_block_cache(&cache);
        }
        opts.set_block_based_table_factory(&block_based_options);
        opts
    }
}
