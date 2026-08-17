use alloy_primitives::U256;
use anyhow::anyhow;
use display_json::DebugAsJson;
use revm::Database;
use revm::context::TransactTo;

use crate::eth::executor::evm::types::EvmInput;
use crate::eth::executor::evm::types::GAS_MAX_LIMIT;
use crate::eth::executor::evm::types::GeneralRevm;
use crate::eth::storage::ExecutionKind;
use crate::eth::storage::TxCount;
use crate::eth::types::Address;
use crate::eth::types::Block;
use crate::eth::types::BlockNumber;
use crate::eth::types::Bytes;
use crate::eth::types::CallInput;
use crate::eth::types::PendingBlockHeader;
use crate::eth::types::PointInTime;
use crate::eth::types::StratusError;
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
    /// Creates from a call that was sent directly to Stratus with `eth_call` or `eth_estimateGas` for a pending block.
    pub fn from_pending_block(input: CallInput, pending_header: PendingBlockHeader, tx_count: TxCount) -> Self {
        Self {
            from: input.from.unwrap_or(Address::ZERO),
            to: input.to.map_into(),
            value: input.value,
            data: input.data,
            block_number: pending_header.number,
            block_timestamp: *pending_header.timestamp,
            kind: ExecutionKind::CallPending(pending_header.number, tx_count),
        }
    }

    /// Creates from a call that was sent directly to Stratus with `eth_call` or `eth_estimateGas` for a mined block.
    pub fn try_from_mined_block(input: CallInput, block: Block, point_in_time: PointInTime) -> anyhow::Result<Self, StratusError> {
        let kind = match point_in_time {
            PointInTime::Latest => ExecutionKind::CallLatest(block.number()),
            PointInTime::Past(number) => ExecutionKind::CallPast(number),
            PointInTime::Pending => return Err(anyhow!("call execution cannot be created on mined block with PointInTime::Pending").into()),
        };
        Ok(Self {
            from: input.from.unwrap_or(Address::ZERO),
            to: input.to.map_into(),
            value: input.value,
            data: input.data,
            block_number: block.number(),
            block_timestamp: block.header.timestamp,
            kind,
        })
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
}
