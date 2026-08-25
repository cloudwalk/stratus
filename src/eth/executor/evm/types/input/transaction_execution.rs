use alloy_primitives::U256;
use display_json::DebugAsJson;
use revm::Database;
use revm::context::TransactTo;

use crate::eth::executor::evm::types::EvmInput;
use crate::eth::executor::evm::types::GAS_MAX_LIMIT;
use crate::eth::executor::evm::types::GeneralRevm;
use crate::eth::storage::ExecutionKind;
use crate::eth::types::Address;
use crate::eth::types::BlockNumber;
use crate::eth::types::Bytes;
use crate::eth::types::ChainId;
use crate::eth::types::Gas;
use crate::eth::types::Nonce;
use crate::eth::types::PendingBlockHeader;
use crate::eth::types::TransactionInput;
use crate::eth::types::UnixTime;
use crate::eth::types::Wei;
use crate::ext::OptionExt;

/// EVM input data. Usually derived from a transaction or call.
#[derive(DebugAsJson, Clone, Default, serde::Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct TransactionExecutionInput {
    /// Operation party address.
    ///
    /// It can be:
    /// * Transaction signer when executing an `eth_sendRawTransaction`.
    /// * Placeholder when performing an `eth_call`
    /// * Not specified when performing an `eth_call`
    pub from: Address,

    /// Operation counterparty address.
    ///
    /// It can be:
    /// * Contract address when performing a function call.
    /// * Destination account address when transfering funds.
    /// * Not specified when deploying a contract.
    pub to: Option<Address>,

    /// Transfered amount from party to counterparty.
    ///
    /// Present only in native token transfers. When calling a contract function, the value is usually zero.
    pub value: Wei,

    /// Operation data.
    ///
    /// It can be:
    /// * Function ID and parameters when performing a contract function call.
    /// * Not specified when transfering funds.
    /// * Contract bytecode when deploying a contract.
    pub data: Bytes,

    /// Operation party nonce.
    ///
    /// It can be:
    /// * Required when executing an `eth_sendRawTransaction`.
    /// * Not specified when performing an `eth_call`.
    pub nonce: Nonce,

    /// Max gas consumption allowed for the transaction.
    pub gas_limit: Gas,

    /// Gas price paid by each unit of gas consumed by the transaction.
    pub gas_price: u128,

    /// Number of the block where the transaction will be or was included.
    pub block_number: BlockNumber,

    /// Timestamp of the block where the transaction will be or was included.
    pub block_timestamp: UnixTime,

    /// ID of the blockchain where the transaction will be or was included.
    ///
    /// If not specified, it will not be validated.
    pub chain_id: Option<ChainId>,

    pub kind: ExecutionKind,
}

impl TransactionExecutionInput {
    /// Creates from a transaction that was sent to Stratus with `eth_sendRawTransaction` or during Importing.
    pub fn from_eth_transaction(input: &TransactionInput, block_number: BlockNumber, block_timestamp: UnixTime) -> Self {
        Self {
            from: input.signer(),
            to: input.execution_info.to,
            value: input.execution_info.value,
            data: input.execution_info.input.clone(),
            gas_limit: input.execution_info.gas_limit,
            gas_price: input.execution_info.gas_price,
            nonce: input.execution_info.nonce,
            block_number,
            block_timestamp,
            chain_id: input.execution_info.chain_id,
            kind: ExecutionKind::Transaction,
        }
    }
}

impl PartialEq<&PendingBlockHeader> for TransactionExecutionInput {
    fn eq(&self, other: &&PendingBlockHeader) -> bool {
        self.block_number == other.number && self.block_timestamp == *other.timestamp
    }
}

impl EvmInput for TransactionExecutionInput {
    fn fill_block_env<DB: Database, I>(&self, evm: &mut GeneralRevm<DB, I>) {
        evm.block.timestamp = U256::from(*self.block_timestamp);
        evm.block.number = U256::from(self.block_number.as_u64());
        evm.block.basefee = 0;
    }

    fn fill_tx_env<DB: Database, I>(self, evm: &mut GeneralRevm<DB, I>) {
        evm.tx.caller = self.from.into();
        evm.tx.kind = match self.to {
            Some(contract) => TransactTo::Call(contract.into()),
            None => TransactTo::Create,
        };
        evm.tx.gas_limit = GAS_MAX_LIMIT;
        evm.tx.gas_price = 0;
        evm.tx.chain_id = self.chain_id.map_into();
        evm.tx.nonce = self.nonce.into();
        evm.tx.data = self.data.into();
        evm.tx.value = self.value.into();
        evm.tx.gas_priority_fee = None;
    }

    fn kind(&self) -> ExecutionKind {
        self.kind
    }
}
