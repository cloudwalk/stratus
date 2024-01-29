use std::collections::HashMap;

use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ChainId;
use crate::eth::primitives::EcdsaRs;
use crate::eth::primitives::EcdsaV;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Index;
use crate::eth::primitives::Log;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::LogTopic;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionExecutionResult;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::TransactionMined;
use crate::eth::primitives::Wei;

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
    // pub block_timestamp: UnixTime,
    pub output: Bytes,
    pub value: Wei,
    pub r: EcdsaRs,
    pub s: EcdsaRs,
    pub v: EcdsaV,
}

impl PostgresTransaction {
    pub fn into_transaction_mined(self, logs: Vec<PostgresLog>, mut topics: HashMap<Index, Vec<PostgresTopic>>) -> TransactionMined {
        let mined_logs: Vec<LogMined> = logs
            .into_iter()
            .map(|log| {
                let log_idx = log.log_idx;
                log.into_log_mined(topics.remove(&log_idx).unwrap_or_default())
            })
            .collect();
        let inner_logs = mined_logs.iter().map(|log| log.log.clone()).collect();
        let execution = TransactionExecution {
            gas: self.gas.clone(),
            output: self.output,
            block_timestamp_in_secs: 0, //*self.block_timestamp,
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
            execution,
            input,
        }
    }
}

#[derive(Clone)]
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
    pub fn into_log_mined(self, topics: Vec<PostgresTopic>) -> LogMined {
        let topics: Vec<LogTopic> = topics.into_iter().map(LogTopic::from).collect();
        let log = Log {
            data: self.data,
            address: self.address,
            topics,
        };

        LogMined {
            transaction_hash: self.transaction_hash,
            transaction_index: self.transaction_idx,
            block_hash: self.block_hash,
            log_index: self.log_idx,
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
