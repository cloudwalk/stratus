pub use inmemory::InMemoryTemporaryStorage;
pub use journal::JournalBlock;
pub use journal::JournalRecord;
pub use journal::TransactionJournal;

mod inmemory;
mod journal;
mod record_file;

use super::RocksPermanentStorage;
use crate::eth::types::BlockNumber;

pub fn compute_pending_block_number(perm_storage: &RocksPermanentStorage) -> anyhow::Result<BlockNumber> {
    let mined_block_number = perm_storage.read_mined_block_number();
    Ok(if !perm_storage.has_genesis()? && mined_block_number == BlockNumber::ZERO {
        BlockNumber::ZERO
    } else {
        mined_block_number + 1
    })
}
