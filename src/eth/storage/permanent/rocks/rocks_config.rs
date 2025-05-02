use rocksdb::BlockBasedOptions;
use rocksdb::Cache;
use rocksdb::Options;

pub enum CacheSetting {
    /// Enabled cache with the given size in bytes
    Enabled(usize),
    Disabled,
}

#[derive(Debug, Clone, Copy)]
pub enum DbConfig {
    OptimizedPointLookUp,
    OptimizedRangeScan,
    MixedWorkload,
}

impl Default for DbConfig {
    fn default() -> Self {
        Self::MixedWorkload
    }
}

impl DbConfig {
    pub fn to_options(self, cache_setting: CacheSetting, prefix_len: Option<usize>) -> Options {
        let mut opts = Options::default();
        let mut block_based_options = BlockBasedOptions::default();

        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        opts.increase_parallelism(16);

        block_based_options.set_pin_l0_filter_and_index_blocks_in_cache(true);
        block_based_options.set_cache_index_and_filter_blocks(true);
        block_based_options.set_bloom_filter(15.5, false);

        // due to the nature of our application enabling rocks metrics decreases point lookup performance by 5x.
        #[cfg(feature = "rocks_metrics")]
        {
            opts.enable_statistics();
            opts.set_statistics_level(rocksdb::statistics::StatsLevel::ExceptTimeForMutex);
        }

        if let Some(prefix_len) = prefix_len {
            let transform = rocksdb::SliceTransform::create_fixed_prefix(prefix_len);
            block_based_options.set_index_type(rocksdb::BlockBasedIndexType::HashSearch);
            opts.set_memtable_prefix_bloom_ratio(0.15);
            opts.set_prefix_extractor(transform);
        }

        if let CacheSetting::Enabled(cache_size) = cache_setting {
            let block_cache = Cache::new_lru_cache(cache_size / 2);
            let row_cache = Cache::new_lru_cache(cache_size / 2);

            opts.set_row_cache(&row_cache);
            block_based_options.set_block_cache(&block_cache);
        }

        match self {
            DbConfig::OptimizedPointLookUp => {
                block_based_options.set_data_block_hash_ratio(0.3);
                block_based_options.set_data_block_index_type(rocksdb::DataBlockIndexType::BinaryAndHash);
                opts.set_use_direct_reads(true);
                opts.set_memtable_whole_key_filtering(true);
                opts.set_compression_type(rocksdb::DBCompressionType::None);
                
                block_based_options.set_block_size(16 * 1024); // 16KB blocks
                opts.set_target_file_size_base(128 * 1024 * 1024); // 128MB SST files
                opts.set_max_bytes_for_level_base(256 * 1024 * 1024); // 256MB for L1
                opts.set_level_compaction_dynamic_level_bytes(false);
            }
            
            DbConfig::OptimizedRangeScan => {
                block_based_options.set_block_size(64 * 1024); // 64KB blocks
                opts.set_target_file_size_base(512 * 1024 * 1024); // 512MB SST files
                opts.set_max_bytes_for_level_base(1024 * 1024 * 1024); // 1GB for L1
                
                opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
                opts.set_bottommost_compression_type(rocksdb::DBCompressionType::Zstd);
                opts.set_bottommost_compression_options(-14, 32767, 0, 16 * 1024, true);
                opts.set_bottommost_zstd_max_train_bytes(1600 * 1024, true);
                
                opts.set_level_compaction_dynamic_level_bytes(true);
            }
            
            DbConfig::MixedWorkload => {
                opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
                opts.set_bottommost_compression_type(rocksdb::DBCompressionType::Zstd);
                opts.set_bottommost_compression_options(-14, 32767, 0, 16 * 1024, true);
                opts.set_bottommost_zstd_max_train_bytes(1600 * 1024, true);
                
                block_based_options.set_block_size(32 * 1024); // 32KB blocks
                opts.set_target_file_size_base(256 * 1024 * 1024); // 256MB SST files
                opts.set_max_bytes_for_level_base(512 * 1024 * 1024); // 512MB for L1
                opts.set_level_compaction_dynamic_level_bytes(true);
            }
        }

        opts.set_block_based_table_factory(&block_based_options);

        opts
    }
}
