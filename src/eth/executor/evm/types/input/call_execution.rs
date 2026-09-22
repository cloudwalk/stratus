use alloy_primitives::U256;
use display_json::DebugAsJson;
use revm::Database;
use revm::context::TransactTo;

use crate::eth::executor::evm::types::EvmInput;
use crate::eth::executor::evm::types::ExecutionMetricsContext;
use crate::eth::executor::evm::types::GAS_MAX_LIMIT;
use crate::eth::executor::evm::types::GeneralRevm;
use crate::eth::storage::ExecutionKind;
use crate::eth::types::Address;
use crate::eth::types::BlockInfo;
use crate::eth::types::BlockNumber;
use crate::eth::types::Bytes;
use crate::eth::types::CallInput;
use crate::eth::types::UnixTime;
use crate::eth::types::Wei;
use crate::ext::OptionExt;

/// EVM input data. Usually derived from a transaction or call.
#[derive(DebugAsJson, Clone, Default, serde::Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct CallExecutionInput {
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

    /// Number of the block where the transaction will be or was included.
    pub block_number: BlockNumber,

    /// Timestamp of the block where the transaction will be or was included.
    pub block_timestamp: UnixTime,

    pub kind: ExecutionKind,
}

impl CallExecutionInput {
    pub fn create(input: CallInput, block_info: BlockInfo, kind: ExecutionKind) -> Self {
        Self {
            from: input.from.unwrap_or(Address::ZERO),
            to: input.to.map_into(),
            value: input.value,
            data: input.data,
            block_number: block_info.number,
            block_timestamp: *block_info.timestamp,
            kind,
        }
    }
}

impl EvmInput for CallExecutionInput {
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
        evm.tx.chain_id = None;
        evm.tx.nonce = 0;
        evm.tx.data = self.data.into();
        evm.tx.value = self.value.into();
        evm.tx.gas_priority_fee = None;
    }

    fn kind(&self) -> ExecutionKind {
        self.kind
    }

    fn metrics_context(&self) -> ExecutionMetricsContext {
        ExecutionMetricsContext::new(self.kind, &self.to, &self.data)
    }
}
