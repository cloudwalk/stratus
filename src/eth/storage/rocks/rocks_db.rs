use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use rocksdb::Options;
use rocksdb::DB;

use crate::eth::storage::rocks::rocks_config::CacheSetting;
use crate::eth::storage::rocks::rocks_config::DbConfig;

/// Create or open the Database with the configs applied to all column families
///
/// The returned `Options` need to be stored to refer to the DB metrics
#[tracing::instrument(skip_all, fields(path = ?path.as_ref()))]
pub fn create_or_open_db(path: impl AsRef<Path>, cf_configs: &HashMap<&'static str, Options>) -> anyhow::Result<(Arc<DB>, Options)> {
    let path = path.as_ref();

    tracing::debug!("creating settings for each column family");
    let cf_config_iter = cf_configs.iter().map(|(name, opts)| (*name, opts.clone()));

    tracing::debug!("generating options for column families");
    let db_opts = DbConfig::Default.to_options(CacheSetting::Disabled);

    if !path.exists() {
        tracing::warn!(?path, "RocksDB at path doesn't exist, creating a new one there instead");
    }

    let open_db = || DB::open_cf_with_opts(&db_opts, path, cf_config_iter.clone());

    tracing::debug!("attempting to open RocksDB");
    let db = match open_db() {
        Ok(db) => db,
        Err(err) => {
            tracing::error!(?err, "Failed to open RocksDB, trying to repair it to open again...");
            DB::repair(&db_opts, path).context("attempting to repair RocksDB cause it failed to open")?;
            open_db().context("trying to open RocksDB a second time, after repairing")?
        }
    };

    tracing::info!("Successfully opened RocksDB at {:?}", path);
    Ok((Arc::new(db), db_opts))
}
