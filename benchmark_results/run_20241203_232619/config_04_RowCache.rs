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
    Default,
}

impl Default for DbConfig {
    fn default() -> Self {
        Self::Default
    }
}

impl DbConfig {
    pub fn to_options(self, cache_setting: CacheSetting, prefix_len: Option<usize>, _key_len: usize) -> Options {
        let mut opts = Options::default();
        let mut block_based_options = BlockBasedOptions::default();

        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        opts.increase_parallelism(16);

        block_based_options.set_pin_l0_filter_and_index_blocks_in_cache(true);
        block_based_options.set_cache_index_and_filter_blocks(true);
        block_based_options.set_bloom_filter(15.5, true);

        // NOTE: As per the rocks db wiki: "The overhead of statistics is usually small but non-negligible. We usually observe an overhead of 5%-10%."
        #[cfg(feature = "metrics")]
        {
            opts.enable_statistics();
            opts.set_statistics_level(rocksdb::statistics::StatsLevel::ExceptTimeForMutex);
        }

        if let Some(prefix_len) = prefix_len {
            let transform = rocksdb::SliceTransform::create_fixed_prefix(prefix_len);
            opts.set_memtable_prefix_bloom_ratio(0.02);
            opts.set_prefix_extractor(transform);
        }

        if let CacheSetting::Enabled(cache_size) = cache_setting {
            let block_cache = Cache::new_lru_cache(cache_size/2);
            let row_cache = Cache::new_lru_cache(cache_size/2);

            opts.set_row_cache(&row_cache);
            block_based_options.set_block_cache(&block_cache);
        }

        match self {
            DbConfig::OptimizedPointLookUp => {
                block_based_options.set_data_block_hash_ratio(0.5);
                block_based_options.set_data_block_index_type(rocksdb::DataBlockIndexType::BinaryAndHash);
                opts.set_memtable_whole_key_filtering(true);
                opts.set_compression_type(rocksdb::DBCompressionType::None);
            }
            DbConfig::Default => {
                opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
                opts.set_bottommost_compression_type(rocksdb::DBCompressionType::Zstd);
                opts.set_bottommost_compression_options(-14, 32767, 0, 16 * 1024, true); // mostly defaults except max_dict_bytes
                opts.set_bottommost_zstd_max_train_bytes(1600 * 1024, true);
            }
        }

        opts.set_block_based_table_factory(&block_based_options);

        opts
    }
}
