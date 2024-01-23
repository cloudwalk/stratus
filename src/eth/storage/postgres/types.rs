use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ChainId;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Index;
use crate::eth::primitives::Log;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::LogTopic;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::Rs;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionExecutionResult;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::TransactionMined;
use crate::eth::primitives::UnixTime;
use crate::eth::primitives::Wei;
use crate::eth::primitives::V;
use crate::eth::EthError;

pub struct PostgresTransaction {
    pub hash: Hash,
    pub signer_address: Address,
    pub nonce: Nonce,
    pub address_from: Address,
    pub address_to: Option<Address>,
    pub input: Bytes,
    pub gas: Gas,
    pub gas_price: Wei,
    pub idx_in_block: Index,
    pub block_number: BlockNumber,
    pub block_hash: Hash,
    pub result: TransactionExecutionResult,
    pub block_timestamp: UnixTime,
    pub output: Bytes,
    pub value: Wei,
    pub r: Rs,
    pub s: Rs,
    pub v: V,
}

impl PostgresTransaction {
    fn into_transaction_mined(self, logs: Vec<PostgresLog>, topics: Vec<PostgresTopic>) -> TransactionMined {
        let mined_logs: Vec<LogMined> = logs.iter().map(|log| log.into_log_mined(topics.clone())).collect();
        let inner_logs = mined_logs.iter().map(|log| log.log.clone()).collect();
        let execution = TransactionExecution {
            gas: self.gas.clone(),
            output: self.output,
            block_timestamp_in_secs: *self.block_timestamp,
            result: self.result,
            logs: inner_logs,
            // TODO: do this correctly
            changes: vec![],
        };
        let input = TransactionInput {
            chain_id: ChainId::default(),
            hash: self.hash,
            nonce: self.nonce,
            signer: self.signer_address,
            from: self.address_from,
            to: self.address_to,
            input: self.input,
            gas: self.gas,
            gas_price: self.gas_price,
            v: self.v.into(),
            r: self.r.into(),
            s: self.s.into(),
            value: self.value,
        };
        TransactionMined {
            transaction_index: self.idx_in_block,
            block_number: self.block_number,
            block_hash: self.block_hash,
            logs: mined_logs,
            execution: execution,
            input,
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
    fn into_log_mined(&self, topics: Vec<PostgresTopic>) -> LogMined {
        let topics: Vec<LogTopic> = topics.iter().map(|topic| LogTopic::from(topic.clone())).collect();
        let log = Log {
            data: self.data.clone(),
            address: self.address.clone(),
            topics,
        };

        LogMined {
            transaction_hash: self.transaction_hash.clone(),
            transaction_index: self.transaction_idx.clone(),
            block_hash: self.block_hash.clone(),
            log_index: self.log_idx.clone(),
            block_number: self.block_number,
            log,
        }
    }
}

#[derive(Clone)]
pub struct PostgresTopic {
    pub topic: Hash,
    pub transaction_hash: Hash,
    pub transaction_idx: Index,
    pub log_idx: Index,
    pub block_number: BlockNumber,
    pub block_hash: Hash,
}

impl From<PostgresTopic> for LogTopic {
    fn from(value: PostgresTopic) -> Self {
        LogTopic::new(value.topic.into())
    }
}
