use ethers_core::types::TransactionReceipt as EthersReceipt;

use crate::eth::primitives::Address;
use crate::eth::primitives::TransactionMined;
use crate::ext::OptionExt;
use crate::if_else;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(transparent)]
pub struct TransactionReceipt(EthersReceipt);

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
impl From<TransactionMined> for TransactionReceipt {
    fn from(trx: TransactionMined) -> Self {
        let contract_address: Option<Address> = trx.execution.contract_address();

        let receipt = EthersReceipt {
            // receipt specific
            status: Some(if_else!(trx.is_success(), 1, 0).into()),
            contract_address: contract_address.map_into(),

            // transaction
            transaction_hash: trx.input.hash.into(),
            from: trx.input.from.into(),
            to: trx.input.to.map_into(),
            gas_used: Some(trx.input.gas.into()),

            // block
            block_hash: Some(trx.block_hash.into()),
            block_number: Some(trx.block_number.into()),
            transaction_index: trx.index_in_block.into(),

            ..Default::default()
        };
        Self(receipt)
    }
}
