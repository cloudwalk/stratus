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

/// Represents a file in a checkpoint directory
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CheckpointFile {
    /// Relative path of the file from the checkpoint directory
    pub path: String,
    /// Size of the file in bytes
    pub size: u64,
}

/// Manages RocksDB checkpoints for the database
pub struct RocksCheckpoint {
    /// The RocksDB instance
    db: Arc<DB>,
    /// The directory where checkpoints will be stored
    checkpoint_dir: PathBuf,
}

impl RocksCheckpoint {
    /// Creates a new RocksCheckpoint instance
    ///
    /// # Arguments
    ///
    /// * `db` - The RocksDB instance to create checkpoints from
    /// * `checkpoint_dir` - The directory where checkpoints will be stored
    pub fn new(db: Arc<DB>, checkpoint_dir: PathBuf) -> Self {
        Self { db, checkpoint_dir }
    }

    /// Checks if a checkpoint exists at the configured directory
    pub fn checkpoint_exists(&self) -> bool {
        self.checkpoint_dir.exists() && self.checkpoint_dir.is_dir()
    }

    /// Creates a checkpoint of the RocksDB database
    ///
    /// If a checkpoint already exists at the configured directory, this function
    /// will return an error.
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

    /// Lists all files in the checkpoint directory recursively
    ///
    /// Returns a list of files with their relative paths and sizes
    pub fn list_checkpoint_files(&self) -> Result<Vec<CheckpointFile>, StorageError> {
        if !self.checkpoint_exists() {
            warn!(path = ?self.checkpoint_dir, "No checkpoint exists at path, cannot list files");
            return Ok(Vec::new());
        }

        info!(path = ?self.checkpoint_dir, "Listing RocksDB checkpoint files");

        let mut files = Vec::new();
        self.list_directory_files(&self.checkpoint_dir, &mut files)
            .context("Failed to list checkpoint files")
            .map_err(|e| StorageError::RocksError { err: e })?;

        info!(path = ?self.checkpoint_dir, file_count = files.len(), "Listed RocksDB checkpoint files");
        Ok(files)
    }

    /// Helper function to recursively list files in a directory
    fn list_directory_files(&self, current_dir: &PathBuf, files: &mut Vec<CheckpointFile>) -> Result<(), anyhow::Error> {
        for entry in fs::read_dir(current_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                // Recursively list files in subdirectories
                self.list_directory_files(&path, files)?;
            } else {
                // Get the relative path from the base directory (checkpoint_dir)
                let relative_path = path.strip_prefix(&self.checkpoint_dir)?.to_string_lossy().to_string();
                let metadata = fs::metadata(&path)?;

                files.push(CheckpointFile {
                    path: relative_path,
                    size: metadata.len(),
                });
            }
        }

        Ok(())
    }

    /// Cleans up (removes) the checkpoint if it exists
    pub fn cleanup_checkpoint(&self) -> Result<(), StorageError> {
        if !self.checkpoint_exists() {
            warn!(path = ?self.checkpoint_dir, "No checkpoint exists at path, nothing to clean up");
            return Ok(());
        }

        info!(path = ?self.checkpoint_dir, "Removing RocksDB checkpoint");

        fs::remove_dir_all(&self.checkpoint_dir)
            .context("Failed to remove checkpoint directory")
            .map_err(|e| StorageError::RocksError { err: e })?;

        info!(path = ?self.checkpoint_dir, "RocksDB checkpoint removed successfully");
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

        // Clean up the checkpoint
        checkpoint.cleanup_checkpoint().unwrap();

        // After cleanup, no checkpoint should exist
        assert!(!checkpoint.checkpoint_exists());
    }
}
