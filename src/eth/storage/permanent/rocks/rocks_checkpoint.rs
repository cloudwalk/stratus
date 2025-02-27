use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use rocksdb::checkpoint::Checkpoint;
use rocksdb::DB;
use tracing::info;
use tracing::warn;

use crate::eth::primitives::StorageError;

/// Manages RocksDB checkpoints for the database
pub struct RocksCheckpoint {
    /// The RocksDB instance
    db: Arc<DB>,
    /// The directory where checkpoints will be stored
    checkpoint_dir: PathBuf,
}

impl RocksCheckpoint {
    /// Creates a new RocksCheckpoint instance
    pub fn new(db: Arc<DB>, checkpoint_dir: PathBuf) -> Self {
        Self { db, checkpoint_dir }
    }

    /// Checks if a checkpoint exists at the configured directory
    pub fn checkpoint_exists(&self) -> bool {
        self.checkpoint_dir.exists() && self.checkpoint_dir.is_dir()
    }

    /// Creates a checkpoint of the RocksDB database
    pub fn create_checkpoint(&self) -> Result<(), StorageError> {
        if self.checkpoint_exists() {
            warn!(path = ?self.checkpoint_dir, "Checkpoint already exists, skipping creation");
            return Ok(());
        }

        info!(path = ?self.checkpoint_dir, "Creating RocksDB checkpoint");

        // Create the checkpoint directory if it doesn't exist
        if let Some(parent) = self.checkpoint_dir.parent() {
            fs::create_dir_all(parent)
                .context("Failed to create parent directory for checkpoint")
                .map_err(|e| StorageError::RocksError { err: e })?;
        }

        // Create the checkpoint
        let checkpoint = Checkpoint::new(&self.db)
            .context("Failed to create checkpoint object")
            .map_err(|e| StorageError::RocksError { err: e })?;

        checkpoint
            .create_checkpoint(&self.checkpoint_dir)
            .context("Failed to create checkpoint")
            .map_err(|e| StorageError::RocksError { err: e })?;

        info!(path = ?self.checkpoint_dir, "RocksDB checkpoint created successfully");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rocksdb::Options;
    use rocksdb::DB;
    use tempfile::TempDir;

    use super::*;

    fn setup_test_db() -> (Arc<DB>, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test_db");
        let mut options = Options::default();
        options.create_if_missing(true);
        let db = DB::open(&options, &db_path).unwrap();
        (Arc::new(db), temp_dir)
    }

    #[test]
    fn test_checkpoint_lifecycle() {
        let (db, temp_dir) = setup_test_db();
        let checkpoint_dir = temp_dir.path().join("checkpoint");

        let checkpoint = RocksCheckpoint::new(db, checkpoint_dir);

        // Initially, no checkpoint should exist
        assert!(!checkpoint.checkpoint_exists());

        // Create a checkpoint
        checkpoint.create_checkpoint().unwrap();

        // Now a checkpoint should exist
        assert!(checkpoint.checkpoint_exists());
    }
}
