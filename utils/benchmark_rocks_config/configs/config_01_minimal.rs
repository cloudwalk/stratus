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
    pub fn to_options(self, cache_setting: CacheSetting, _prefix_len: Option<usize>, _key_len: usize) -> Options {
        let mut opts = Options::default();
        let mut block_based_options = BlockBasedOptions::default();

        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        opts.increase_parallelism(16);
        block_based_options.set_pin_l0_filter_and_index_blocks_in_cache(true);
        block_based_options.set_cache_index_and_filter_blocks(true);
        block_based_options.set_ribbon_filter(15.5);

        // NOTE: As per the rocks db wiki: "The overhead of statistics is usually small but non-negligible. We usually observe an overhead of 5%-10%."
        #[cfg(feature = "metrics")]
        {
            opts.enable_statistics();
            opts.set_statistics_level(rocksdb::statistics::StatsLevel::ExceptTimeForMutex);
        }

        if let CacheSetting::Enabled(cache_size) = cache_setting {
            let cache = Cache::new_lru_cache(cache_size);
            block_based_options.set_block_cache(&cache);
            block_based_options.set_cache_index_and_filter_blocks(true);
        }

        match self {
            DbConfig::OptimizedPointLookUp => {
                opts.set_compression_type(rocksdb::DBCompressionType::None);
            }
            DbConfig::Default => {
                opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
                opts.set_bottommost_compression_type(rocksdb::DBCompressionType::Zstd);
                opts.set_bottommost_compression_options(-14, 32767, 0, 16 * 1024, true); // mostry defaults except max_dict_bytes
                opts.set_bottommost_zstd_max_train_bytes(1600 * 1024, true);
            }
        }

        opts.set_block_based_table_factory(&block_based_options);
        opts
    }
}
