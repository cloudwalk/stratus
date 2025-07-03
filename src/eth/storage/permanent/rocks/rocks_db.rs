use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Context;
use rocksdb::Options;
use rocksdb::DB;

use super::rocks_config::CacheSetting;
use super::rocks_config::DbConfig;
#[cfg(feature = "metrics")]
use crate::infra::metrics;

/// Open (or create) the Database with the configs applied to all column families.
///
/// This function creates all the CFs in the database.
///
/// The returned `Options` **need** to be stored to refer to the DB metrics!
#[tracing::instrument(skip_all, fields(path = ?path.as_ref()))]
pub fn create_or_open_db(path: impl AsRef<Path>, cf_configs: &BTreeMap<&'static str, Options>) -> anyhow::Result<(&'static Arc<DB>, Options)> {
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
    let instant = Instant::now();
    let db = match open_db() {
        Ok(db) => db,
        Err(e) => {
            tracing::error!(reason = ?e, "failed to open RocksDB, trying to repair it to open again...");
            DB::repair(&db_opts, path).context("attempting to repair RocksDB cause it failed to open")?;
            open_db().context("trying to open RocksDB a second time, after repairing")?
        }
    };

    let waited_for = instant.elapsed();
    tracing::info!(?waited_for, db_path = ?path, "successfully opened RocksDB");

    #[cfg(feature = "metrics")]
    {
        let db_name = path.file_name().with_context(|| format!("invalid db path without name '{path:?}'"))?.to_str();
        metrics::set_rocks_last_startup_delay_millis(waited_for.as_millis() as u64, db_name);
    }

    // we leak here to create a 'static reference to the db. This is safe because this function is only called once
    // and the db is used until the program exits.
    Ok((Box::leak(Box::new(Arc::new(db))), db_opts))
}
