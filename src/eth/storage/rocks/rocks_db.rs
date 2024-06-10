use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::anyhow;
use rocksdb::backup::BackupEngine;
use rocksdb::backup::BackupEngineOptions;
use rocksdb::Env;
use rocksdb::Options;
use rocksdb::DB;

use crate::eth::storage::rocks::rocks_config::CacheSetting;
use crate::eth::storage::rocks::rocks_config::DbConfig;

/// Create or open the Database with the configs applied to all column families
///
/// The returned `Options` need to be stored to refer to the DB metrics
#[tracing::instrument(skip_all)]
pub fn create_or_open_db(path: impl AsRef<Path>, cf_configs: &HashMap<&'static str, Options>) -> anyhow::Result<(Arc<DB>, Options)> {
    let path = path.as_ref();

    // settings for each Column Family to be created
    let cf_config_iter = cf_configs.iter().map(|(name, opts)| (*name, opts.clone()));

    // options for the "default" column family (used only to refer to the DB metrics)
    let db_opts = DbConfig::Default.to_options(CacheSetting::Disabled);

    let open_db = || DB::open_cf_with_opts(&db_opts, path, cf_config_iter.clone());

    let db = match open_db() {
        Ok(db) => db,
        Err(e) => {
            tracing::error!("Failed to open RocksDB: {}", e);
            DB::repair(&db_opts, path)?;
            open_db()?
        }
    }; // XXX in case of corruption, use DB

    Ok((Arc::new(db), db_opts))
}

#[tracing::instrument(skip_all)]
pub fn create_new_backup(db: &DB) -> anyhow::Result<()> {
    tracing::info!("Creating new DB backup");
    let mut backup_engine = backup_engine(db)?;
    backup_engine.create_new_backup(db)?;
    backup_engine.purge_old_backups(2)?;
    Ok(())
}

fn backup_engine(db: &DB) -> anyhow::Result<BackupEngine> {
    let db_path = db.path().to_str().ok_or(anyhow!("Invalid path"))?;
    let backup_path = format!("{db_path}backup");
    let backup_opts = BackupEngineOptions::new(backup_path)?;

    let backup_env = Env::new()?;
    Ok(BackupEngine::open(&backup_opts, &backup_env)?)
}
