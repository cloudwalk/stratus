use std::sync::Arc;

use chrono::Utc;
use ethereum_types::Bloom;
use ethereum_types::H256;
use ethereum_types::H64;
use ethereum_types::U256;
use ethers_core::types::Block as EthersBlock;
use ethers_core::types::Bytes as EthersBytes;

use crate::eth::primitives::Address;
use crate::eth::primitives::Execution;
use crate::eth::primitives::Hash;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::BlockNumberStorage;
use crate::eth::EthError;

pub struct BlockMiner {
    storage: Arc<dyn BlockNumberStorage>,
}

impl BlockMiner {
    pub fn new(storage: Arc<dyn BlockNumberStorage>) -> Self {
        Self { storage }
    }

    pub fn mine(&mut self, transaction: TransactionInput, execution: Execution) -> Result<TransactionMined, EthError> {
        let number = self.storage.increment_block_number()?;

        // mine logs
        let bloom = Bloom::default();
        // if execution.is_success() {
        //     for log in &execution.logs {}
        // }

        let block = EthersBlock {
            // block: identifiers
            hash: Some(H256::random()), // TODO: how hash is created?
            number: Some(number.into()),

            // block: relation with other blocks
            uncles_hash: Hash::EMPTY_UNCLE.into(),
            uncles: Vec::new(),
            parent_beacon_block_root: None,

            // mining: identifiers
            timestamp: Utc::now().timestamp().into(),
            author: Some(Address::COINBASE.into()),

            // minining: difficulty
            difficulty: U256::zero(),
            total_difficulty: Some(U256::zero()),
            nonce: Some(H64::zero()),

            // mining: gas
            gas_limit: transaction.gas_limit.clone().into(),
            gas_used: execution.gas.clone().into(),
            base_fee_per_gas: Some(U256::zero()),
            blob_gas_used: None,
            excess_blob_gas: None,

            // data
            logs_bloom: Some(bloom),
            extra_data: EthersBytes::default(),

            ..Default::default() // seal_fields: todo!(),
                                 // transactions: todo!(),
                                 // size: todo!(),
                                 // mix_hash: todo!(),
                                 // withdrawals_root: todo!(),
                                 // withdrawals: todo!(),
                                 // other: todo!(),
                                 // parent_hash: todo!(),
                                 // state_root: todo!(),
                                 // transactions_root: todo!(),
                                 // receipts_root: todo!(),
        };

        let mined = TransactionMined::new(transaction, execution, block.into());
        Ok(mined)
    }
}
