use std::path::Path;
use std::sync::Arc;

use anyhow::anyhow;
use rocksdb::backup::BackupEngine;
use rocksdb::backup::BackupEngineOptions;
use rocksdb::Env;
use rocksdb::Options;
use rocksdb::DB;

/// Create or open the Database with the configs applied to all column families
pub fn create_or_open_db<CfDescriptorIter, CfName>(path: &Path, db_opts: &Options, cf_descriptor_iter: CfDescriptorIter) -> anyhow::Result<Arc<DB>>
where
    CfDescriptorIter: Iterator<Item = (CfName, Options)> + Clone,
    CfName: AsRef<str>,
{
    let open_db = || DB::open_cf_with_opts(db_opts, path, cf_descriptor_iter.clone());

    let db = match open_db() {
        Ok(db) => db,
        Err(e) => {
            tracing::error!("Failed to open RocksDB: {}", e);
            DB::repair(db_opts, path)?;
            open_db()?
        }
    }; // XXX in case of corruption, use DB

    Ok(Arc::new(db))
}

pub fn create_new_backup(db: &DB) -> anyhow::Result<()> {
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
