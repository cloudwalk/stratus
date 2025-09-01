use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Context;
use rocksdb::DB;
use rocksdb::Options;

use super::rocks_config::CacheSetting;
use super::rocks_config::DbConfig;
use super::rocks_state::generate_cf_options_map;
#[cfg(feature = "metrics")]
use crate::infra::metrics;

/// Drop a specific column family if it exists in the database.
/// This is used for database migrations when we need to remove obsolete column families.
fn drop_column_family_if_exists(path: impl AsRef<Path>, cf_name: &str) -> anyhow::Result<()> {
    let path = path.as_ref();
    
    if !path.exists() {
        tracing::debug!("Database path doesn't exist, skipping column family drop");
        return Ok(());
    }

    tracing::info!(cf_name = cf_name, "Checking if column family exists to drop");
    
    // First, try to open the database to check existing column families
    let db_opts = DbConfig::Default.to_options(CacheSetting::Disabled);
    
    // List existing column families
    let existing_cfs = match DB::list_cf(&db_opts, path) {
        Ok(cfs) => cfs,
        Err(e) => {
            tracing::warn!(error = ?e, "Failed to list column families, database might be corrupted or new");
            return Ok(());
        }
    };
    
    if !existing_cfs.contains(&cf_name.to_string()) {
        tracing::debug!(cf_name = cf_name, "Column family doesn't exist, nothing to drop");
        return Ok(());
    }
    
    tracing::info!(cf_name = cf_name, "Column family exists, proceeding to drop it");
    
    // Generate the proper column family configurations for existing CFs
    let cf_options_map = generate_cf_options_map(None);
    
    // Create options for all existing column families, using proper configs for known ones
    let mut cf_opts_vec = Vec::new();
    for cf_name_existing in &existing_cfs {
        let opts = if let Some(configured_opts) = cf_options_map.get(cf_name_existing.as_str()) {
            configured_opts.clone()
        } else {
            // For unknown column families (like block_changes), use default options
            DbConfig::Default.to_options(CacheSetting::Disabled)
        };
        cf_opts_vec.push((cf_name_existing.as_str(), opts));
    }
    
    // Open database with all existing column families using their proper configurations
    let db = DB::open_cf_with_opts(&db_opts, path, cf_opts_vec)
        .with_context(|| format!("Failed to open database to drop column family '{}'", cf_name))?;
    
    // Drop the specific column family
    db.drop_cf(cf_name)
        .with_context(|| format!("Failed to drop column family '{}'", cf_name))?;
    
    tracing::info!(cf_name = cf_name, "Successfully dropped column family");
    Ok(())
}

/// Open (or create) the Database with the configs applied to all column families.
///
/// This function creates all the CFs in the database.
///
/// The returned `Options` **need** to be stored to refer to the DB metrics!
#[tracing::instrument(skip_all, fields(path = ?path.as_ref()))]
pub fn create_or_open_db(path: impl AsRef<Path>, cf_configs: &BTreeMap<&'static str, Options>) -> anyhow::Result<(&'static Arc<DB>, Options)> {
    let path = path.as_ref();

    // Drop the obsolete block_changes column family if it exists
    drop_column_family_if_exists(path, "block_changes")
        .context("Failed to drop obsolete block_changes column family")?;

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
