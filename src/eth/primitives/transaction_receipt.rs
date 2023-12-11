use ethereum_types::U64;
use ethers_core::types::TransactionReceipt as EthersReceipt;

use crate::eth::primitives::Address;
use crate::eth::primitives::TransactionExecutionResult;
use crate::eth::primitives::TransactionMined;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(transparent)]
pub struct TransactionReceipt(EthersReceipt);

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
impl From<TransactionMined> for TransactionReceipt {
    fn from(value: TransactionMined) -> Self {
        let transaction_input = value.transaction_input;
        let execution = value.execution;
        let block = value.block;
        let contract_address: Option<Address> = execution.contract_address();

        let receipt = EthersReceipt {
            block_hash: Some(block.hash.into()),
            block_number: Some(block.number.into()),
            transaction_hash: transaction_input.hash.into(),
            from: transaction_input.from.into(),
            to: transaction_input.to.map(|x| x.into()),
            gas_used: Some(execution.gas_used.into()),
            status: match execution.result {
                TransactionExecutionResult::Commited { .. } => Some(U64::one()),
                TransactionExecutionResult::Reverted { .. } => Some(U64::zero()),
                TransactionExecutionResult::Halted { .. } => Some(U64::zero()),
            },
            contract_address: contract_address.map(|x| x.into()),
            ..Default::default()
        };
        Self(receipt)
    }
}
