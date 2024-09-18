//! Revert block binary.
//!
//! Given a block number, it loads the Rocks database and tries to revert the state to that block.
//!
//! By reverting the state to a previous block, the final state must be the same as when that block
//! was just processed, that is, before the next ones were processed.

use std::cmp::Ordering;
use std::time::Duration;

use anyhow::bail;
use stratus::config::RocksRevertToBlockConfig;
use stratus::eth::storage::PermanentStorage;
use stratus::eth::storage::RocksPermanentStorage;
use stratus::utils::DropTimer;
use stratus::GlobalServices;
#[cfg(all(not(target_env = "msvc"), any(feature = "jemalloc", feature = "jeprof")))]
use tikv_jemallocator::Jemalloc;

#[cfg(all(not(target_env = "msvc"), any(feature = "jemalloc", feature = "jeprof")))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;
fn main() -> anyhow::Result<()> {
    let global_services = GlobalServices::<RocksRevertToBlockConfig>::init();
    run(global_services.config)
}

fn run(config: RocksRevertToBlockConfig) -> anyhow::Result<()> {
    let _timer = DropTimer::start("rocks-revert-to-block");

    let rocks = RocksPermanentStorage::new(config.rocks_path_prefix, Duration::from_secs(30))?;

    let target_block = config.block_number;
    let current_block = rocks.read_mined_block_number()?.as_u64();

    match target_block.cmp(&current_block) {
        Ordering::Less => {
            // ok!
        }
        Ordering::Equal => {
            tracing::warn!("block target is equal to current block, reprocessing current state regardless");
        }
        Ordering::Greater => {
            bail!("block number to revert to is greater than the current block number, exiting");
        }
    }

    if let Err(err) = rocks.revert_state_to_block(target_block.into()) {
        tracing::error!(target_block, reason = ?err, "failed to revert block state to target block");
    }

    Ok(())
}
