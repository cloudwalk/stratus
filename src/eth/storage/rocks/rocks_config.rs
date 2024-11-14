use rocksdb::BlockBasedOptions;
use rocksdb::Cache;
use rocksdb::CuckooTableOptions;
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
    pub fn to_options(self, _cache_setting: CacheSetting) -> Options {
        let mut opts = Options::default();

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
            DbConfig::OptimizedPointLookUp => {
                let cuckoo = CuckooTableOptions::default();
                opts.set_allow_mmap_reads(true);
                opts.set_compression_type(rocksdb::DBCompressionType::None);
                opts.set_cuckoo_table_factory(&cuckoo);
            }
            DbConfig::Default => {

                let mut block_based_options = BlockBasedOptions::default();

                block_based_options.set_pin_l0_filter_and_index_blocks_in_cache(true);
                block_based_options.set_cache_index_and_filter_blocks(true);
                block_based_options.set_ribbon_filter(15.5);

                opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
                opts.set_bottommost_compression_type(rocksdb::DBCompressionType::Zstd);
                opts.set_bottommost_compression_options(-14, 32767, 0, 16 * 1024, true); // mostly defaults except max_dict_bytes
                opts.set_bottommost_zstd_max_train_bytes(1600 * 1024, true);
            }
        }

        opts
    }
}
