use std::collections::HashMap;

use anyhow::Context;

use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ChainId;
use crate::eth::primitives::CodeHash;
use crate::eth::primitives::EcdsaRs;
use crate::eth::primitives::EcdsaV;
use crate::eth::primitives::Execution;
use crate::eth::primitives::ExecutionResult;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Index;
use crate::eth::primitives::Log;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::LogTopic;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotValue;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::TransactionMined;
use crate::eth::primitives::UnixTime;
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
    pub result: ExecutionResult,
    // pub block_timestamp: UnixTime,
    pub output: Bytes,
    pub value: Wei,
    pub r: EcdsaRs,
    pub s: EcdsaRs,
    pub v: EcdsaV,
    pub chain_id: Option<ChainId>,
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
        let execution = Execution {
            gas: self.gas.clone(),
            output: self.output,
            block_timestamp: UnixTime::ZERO, //*self.block_timestamp,
            result: self.result,
            logs: inner_logs,
            // TODO: do this correctly
            changes: vec![],
            execution_costs_applied: true,
        };
        let input = TransactionInput {
            chain_id: self.chain_id,
            hash: self.hash,
            nonce: self.nonce,
            signer: self.signer_address,
            from: self.address_from,
            to: self.address_to,
            input: self.input,
            gas_limit: self.gas,
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

#[derive(Default)]
pub struct TransactionBatch {
    pub hash: Vec<Hash>,
    pub signer: Vec<Address>,
    pub nonce: Vec<Nonce>,
    pub from: Vec<Address>,
    pub to: Vec<Address>,
    pub input: Vec<Bytes>,
    pub output: Vec<Bytes>,
    pub gas: Vec<Gas>,
    pub gas_price: Vec<Wei>,
    pub index: Vec<Index>,
    pub block_number: Vec<BlockNumber>,
    pub block_hash: Vec<Hash>,
    pub v: Vec<[u8; 8]>,
    pub r: Vec<[u8; 32]>,
    pub s: Vec<[u8; 32]>,
    pub value: Vec<Wei>,
    pub result: Vec<String>,
    pub chain_id: Vec<Option<ChainId>>,
}

impl TransactionBatch {
    pub fn push(&mut self, transaction: TransactionMined) {
        self.hash.push(transaction.input.hash);
        self.signer.push(transaction.input.signer.clone());
        self.nonce.push(transaction.input.nonce);
        self.from.push(transaction.input.signer);
        self.to.push(transaction.input.to.unwrap_or_default());
        self.input.push(transaction.input.input);
        self.output.push(transaction.execution.output);
        self.gas.push(transaction.execution.gas);
        self.gas_price.push(transaction.input.gas_price);
        self.index.push(transaction.transaction_index);
        self.block_number.push(transaction.block_number);
        self.block_hash.push(transaction.block_hash);
        self.v.push(<[u8; 8]>::from(transaction.input.v));
        self.r.push(<[u8; 32]>::from(transaction.input.r));
        self.s.push(<[u8; 32]>::from(transaction.input.s));
        self.value.push(transaction.input.value);
        self.result.push(transaction.execution.result.to_string());
        self.chain_id.push(transaction.input.chain_id);
    }
}

#[derive(Default)]
pub struct LogBatch {
    pub address: Vec<Address>,
    pub data: Vec<Bytes>,
    pub transaction_hash: Vec<Hash>,
    pub transaction_index: Vec<Index>,
    pub log_index: Vec<Index>,
    pub block_number: Vec<BlockNumber>,
    pub block_hash: Vec<Hash>,
}

impl LogBatch {
    pub fn push(&mut self, log: LogMined) {
        self.address.push(log.log.address);
        self.data.push(log.log.data);
        self.transaction_hash.push(log.transaction_hash);
        self.log_index.push(log.log_index);
        self.transaction_index.push(log.transaction_index);
        self.block_number.push(log.block_number);
        self.block_hash.push(log.block_hash);
    }
}

#[derive(Default)]
pub struct TopicBatch {
    pub topic: Vec<LogTopic>,
    pub transaction_hash: Vec<Hash>,
    pub transaction_index: Vec<Index>,
    pub log_index: Vec<Index>,
    pub index: Vec<i32>,
    pub block_number: Vec<BlockNumber>,
    pub block_hash: Vec<Hash>,
}

impl TopicBatch {
    #[allow(clippy::too_many_arguments)]
    pub fn push(
        &mut self,
        topic: LogTopic,
        idx: usize,
        tx_hash: Hash,
        tx_idx: Index,
        log_idx: Index,
        block_number: BlockNumber,
        block_hash: Hash,
    ) -> anyhow::Result<()> {
        self.topic.push(topic);
        self.index.push(i32::try_from(idx).context("failed to convert topic idx")?);
        self.block_hash.push(block_hash);
        self.log_index.push(log_idx);
        self.block_number.push(block_number);
        self.transaction_hash.push(tx_hash);
        self.transaction_index.push(tx_idx);
        Ok(())
    }
}

#[derive(Default)]
pub struct AccountBatch {
    pub address: Vec<Address>,
    pub new_nonce: Vec<Nonce>,
    pub new_balance: Vec<Wei>,
    pub bytecode: Vec<Option<Bytes>>,
    pub block_number: Vec<BlockNumber>,
    pub original_nonce: Vec<Nonce>,
    pub original_balance: Vec<Wei>,
    pub code_hash: Vec<CodeHash>,
}

impl AccountBatch {
    #[allow(clippy::too_many_arguments)]
    pub fn push(
        &mut self,
        address: Address,
        new_nonce: Nonce,
        new_balance: Wei,
        bytecode: Option<Bytes>,
        block_number: BlockNumber,
        original_nonce: Nonce,
        original_balance: Wei,
        code_hash: CodeHash,
    ) {
        self.address.push(address);
        self.new_nonce.push(new_nonce);
        self.new_balance.push(new_balance);
        self.bytecode.push(bytecode);
        self.block_number.push(block_number);
        self.original_nonce.push(original_nonce);
        self.original_balance.push(original_balance);
        self.code_hash.push(code_hash);
    }
}

#[derive(Default)]
pub struct HistoricalNonceBatch {
    pub address: Vec<Address>,
    pub nonce: Vec<Nonce>,
    pub block_number: Vec<BlockNumber>,
}

impl HistoricalNonceBatch {
    pub fn push(&mut self, address: Address, nonce: Nonce, block_number: BlockNumber) {
        self.address.push(address);
        self.nonce.push(nonce);
        self.block_number.push(block_number);
    }
}

#[derive(Default)]
pub struct HistoricalBalanceBatch {
    pub address: Vec<Address>,
    pub balance: Vec<Wei>,
    pub block_number: Vec<BlockNumber>,
}

impl HistoricalBalanceBatch {
    pub fn push(&mut self, address: Address, balance: Wei, block_number: BlockNumber) {
        self.address.push(address);
        self.balance.push(balance);
        self.block_number.push(block_number);
    }
}

#[derive(Default, Debug)]
pub struct SlotBatch {
    pub index: Vec<SlotIndex>,
    pub value: Vec<SlotValue>,
    pub address: Vec<Address>,
    pub block_number: Vec<BlockNumber>,
    pub original_value: Vec<SlotValue>,
}

impl SlotBatch {
    pub fn push(&mut self, address: Address, slot_idx: SlotIndex, new_value: SlotValue, block_number: BlockNumber, original_value: SlotValue) {
        self.address.push(address);
        self.index.push(slot_idx);
        self.value.push(new_value);
        self.block_number.push(block_number);
        self.original_value.push(original_value);
    }
}

#[derive(Default)]
pub struct HistoricalSlotBatch {
    pub address: Vec<Address>,
    pub index: Vec<SlotIndex>,
    pub value: Vec<SlotValue>,
    pub block_number: Vec<BlockNumber>,
}

impl HistoricalSlotBatch {
    pub fn push(&mut self, address: Address, index: SlotIndex, value: SlotValue, block_number: BlockNumber) {
        self.address.push(address);
        self.index.push(index);
        self.value.push(value);
        self.block_number.push(block_number);
    }
}
