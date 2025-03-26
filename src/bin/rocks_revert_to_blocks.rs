//! Revert block binary.
//!
//! Given a block number, it loads the Rocks database and tries to revert the state to that block.
//!
//! By reverting the state to a previous block, the final state must be the same as when that block
//! was just processed, that is, before the next ones were processed.

use std::time::Duration;

use stratus::config::RocksRevertToBlockConfig;
use stratus::eth::storage::permanent::rocks::RocksPermanentStorage;
use stratus::utils::DropTimer;
use stratus::GlobalServices;

fn main() -> anyhow::Result<()> {
    let global_services = GlobalServices::<RocksRevertToBlockConfig>::init();
    run(global_services.config)
}

fn run(config: RocksRevertToBlockConfig) -> anyhow::Result<()> {
    let _timer = DropTimer::start("rocks-revert-to-block");

    let rocks = RocksPermanentStorage::new(config.rocks_path_prefix, Duration::from_secs(10), Some(0.1), true)?;

    if let Err(err) = rocks.revert_state_to_block_batched(config.block_number.into()) {
        tracing::error!(target_block = config.block_number, reason = ?err, "failed to revert block state to target block");
    }

    Ok(())
}
