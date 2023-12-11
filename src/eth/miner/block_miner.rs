use std::sync::Arc;

use ethereum_types::Bloom;
use ethereum_types::H256;
use ethers_core::types::Block as EthersBlock;

use crate::eth::primitives::TransactionExecution;
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

    pub fn mine(&mut self, transaction: TransactionInput, execution: TransactionExecution) -> Result<TransactionMined, EthError> {
        let number = self.storage.increment_block_number()?;

        let block = EthersBlock {
            // block identifiers
            hash: Some(H256::random()), // TODO: how hash is created?
            number: Some(number.into()),

            // gas consumption
            gas_limit: transaction.gas_limit.clone().into(),
            gas_used: execution.gas_used.clone().into(),

            // transactions
            logs_bloom: Some(Bloom::zero()), // TODO: need to collect logs and implement

            // missing data
            ..Default::default()
        };

        let mined = TransactionMined::new(transaction, execution, block.into());
        Ok(mined)
    }
}
