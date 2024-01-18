use crate::eth::EthError;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Index;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionMined;

pub struct PostgresTransaction {
    pub hash: Hash,
    pub signer_address: Address,
    pub nonce: Nonce,
    pub address_from: Address,
    pub address_to: Option<Address>,
    pub input: Bytes,
    pub gas: Gas,
    pub idx_in_block: Index,
    pub block_number: BlockNumber,
    pub block_hash: Hash,
}

impl PostgresTransaction {
    fn into_transaction_mined(&self, logs: Vec<PostgresLog>) -> TransactionMined {
        let mined_logs: Vec<LogMined> = logs.iter().map(|log| log.into_log_mined())
       TransactionMined {
            transaction_index: self.idx_in_block,
            block_number: self.block_number,
            block_hash: self.block_hash,
            logs: mined_logs,
        }

    }
}

pub struct PostgresLog {
    pub address: Address,
    pub data: Bytes,
    pub transaction_hash: Hash,
    pub transaction_idx: Index,
    pub log_idx: Index,
    pub block_number: BlockNumber,
    pub block_hash: Hash,
}

impl PostgresLog {
    fn into_log_mined(&self) -> LogMined {
        LogMined {
            transaction_hash: self.transaction_hash,
            transaction_index: self.transaction_idx,
            block_hash: self.block_hash,
            log_index: self.log_idx,
            log: self.
        }
    }
}

pub struct PostgresTopic {
    pub topic: Bytes,
    pub transaction_hash: Hash,
    pub transaction_idx: Index,
    pub log_idx: Index,
    pub block_number: BlockNumber,
    pub block_hash: Hash,
}

impl PostgresTopic {
    fn into_topic_mined(&self) -> Topic {
        
    }
}