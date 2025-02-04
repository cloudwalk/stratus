use alloy_consensus::Transaction;
use alloy_rpc_types_trace::geth::GethDebugTracingOptions;
use display_json::DebugAsJson;

use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::CallInput;
use crate::eth::primitives::ChainId;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::ExternalTransaction;
use crate::eth::primitives::ExternalTransactionExecution;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::PendingBlockHeader;
use crate::eth::primitives::PointInTime;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::TransactionMined;
use crate::eth::primitives::TransactionStage;
use crate::eth::primitives::UnixTime;
use crate::eth::primitives::Wei;
use crate::ext::not;
use crate::ext::OptionExt;
use crate::if_else;
use crate::log_and_err;

/// EVM input data. Usually derived from a transaction or call.
#[derive(DebugAsJson, Clone, Default, serde::Serialize, serde::Deserialize, fake::Dummy, PartialEq)]
pub struct EvmInput {
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
    pub nonce: Option<Nonce>,

    /// Max gas consumption allowed for the transaction.
    pub gas_limit: Gas,

    /// Gas price paid by each unit of gas consumed by the transaction.
    pub gas_price: Wei,

    /// Number of the block where the transaction will be or was included.
    pub block_number: BlockNumber,

    /// Timestamp of the block where the transaction will be or was included.
    pub block_timestamp: UnixTime,

    /// Point-in-time from where accounts and slots will be read.
    pub point_in_time: PointInTime,

    /// ID of the blockchain where the transaction will be or was included.
    ///
    /// If not specified, it will not be validated.
    pub chain_id: Option<ChainId>,
}

impl EvmInput {
    /// Creates from a transaction that was sent directly to Stratus with `eth_sendRawTransaction`.
    pub fn from_eth_transaction(input: &TransactionInput, pending_header: &PendingBlockHeader) -> Self {
        Self {
            from: input.signer,
            to: input.to,
            value: input.value,
            data: input.input.clone(),
            gas_limit: Gas::MAX,
            gas_price: Wei::ZERO,
            nonce: Some(input.nonce),
            block_number: pending_header.number,
            block_timestamp: *pending_header.timestamp,
            point_in_time: PointInTime::Pending,
            chain_id: input.chain_id,
        }
    }

    /// Creates from a call that was sent directly to Stratus with `eth_call` or `eth_estimateGas`.
    ///
    /// # Errors:
    ///
    /// If `point_in_time` is `MinedPast` it's required that `mined_block` is `Some`, otherwise, this function returns an error.
    pub fn from_eth_call(input: CallInput, point_in_time: PointInTime, pending_header: PendingBlockHeader, mined_block: Option<Block>) -> anyhow::Result<Self> {
        Ok(Self {
            from: input.from.unwrap_or(Address::ZERO),
            to: input.to.map_into(),
            value: input.value,
            data: input.data,
            gas_limit: Gas::MAX,
            gas_price: Wei::ZERO,
            nonce: None,
            block_number: match point_in_time {
                PointInTime::Mined | PointInTime::Pending => pending_header.number,
                PointInTime::MinedPast(number) => number,
            },
            block_timestamp: match point_in_time {
                PointInTime::Mined | PointInTime::Pending => *pending_header.timestamp,
                PointInTime::MinedPast(_) => match mined_block {
                    Some(block) => block.header.timestamp,
                    None => return log_and_err!("failed to create EvmInput: couldn't determine mined block timestamp"),
                },
            },
            point_in_time,
            chain_id: None,
        })
    }

    /// Creates a transaction that was executed in an external blockchain and imported to Stratus.
    ///
    /// Successful external transactions executes with max gas and zero gas price to ensure we will have the same execution result.
    // TODO: improve before merging(validate conversions)
    pub fn from_external(tx: &ExternalTransaction, receipt: &ExternalReceipt, block_number: BlockNumber, block_timestamp: UnixTime) -> anyhow::Result<Self> {
        Ok(Self {
            from: tx.from.into(),
            to: tx.inner.to().map_into(),
            value: tx.inner.value().into(),
            data: tx.inner.input().clone().into(),
            nonce: Some(tx.inner.nonce().into()),
            gas_limit: if_else!(receipt.is_success(), Gas::MAX, tx.inner.gas_limit().into()),
            gas_price: if_else!(receipt.is_success(), Wei::ZERO, tx.inner.gas_price().map_into().unwrap_or(Wei::ZERO)),
            point_in_time: PointInTime::Pending,
            block_number,
            block_timestamp,
            chain_id: tx.inner.chain_id().map(Into::into),
        })
    }

    /// Checks if the input is a contract call.
    ///
    /// It is when there is a `to` address and the `data` field is also populated.
    pub fn is_contract_call(&self) -> bool {
        self.to.is_some() && not(self.data.is_empty())
    }
}

impl PartialEq<(&TransactionInput, &PendingBlockHeader)> for EvmInput {
    fn eq(&self, other: &(&TransactionInput, &PendingBlockHeader)) -> bool {
        self.block_number == other.1.number
            && self.block_timestamp == *other.1.timestamp
            && self.chain_id == other.0.chain_id
            && self.data == other.0.input
            && self.from == other.0.signer
            && self.nonce.is_some_and(|inner| inner == other.0.nonce)
            && self.value == other.0.value
            && self.to == other.0.to
    }
}

impl TryFrom<ExternalTransactionExecution> for EvmInput {
    type Error = anyhow::Error;
    fn try_from(value: ExternalTransactionExecution) -> Result<Self, Self::Error> {
        EvmInput::from_external(
            &value.tx,
            &value.receipt,
            value.receipt.block_number(),
            value.evm_execution.execution.block_timestamp,
        )
    }
}

impl From<TransactionMined> for EvmInput {
    fn from(value: TransactionMined) -> Self {
        Self {
            from: value.input.signer,
            to: value.input.to,
            value: value.input.value,
            data: value.input.input,
            nonce: Some(value.input.nonce),
            gas_limit: value.input.gas_limit,
            gas_price: value.input.gas_price,
            block_number: value.block_number,
            block_timestamp: value.execution.block_timestamp,
            point_in_time: PointInTime::MinedPast(value.block_number),
            chain_id: value.input.chain_id,
        }
    }
}

impl TryFrom<TransactionStage> for EvmInput {
    type Error = anyhow::Error;

    fn try_from(value: TransactionStage) -> Result<Self, Self::Error> {
        match value {
            TransactionStage::Executed(TransactionExecution::External(tx)) => tx.try_into(),
            TransactionStage::Executed(TransactionExecution::Local(tx)) => Ok(tx.evm_input),
            TransactionStage::Mined(tx) => Ok(tx.into()),
        }
    }
}

pub struct InspectorInput {
    pub tx_hash: Hash,
    pub opts: GethDebugTracingOptions,
    pub trace_unsuccessful_only: bool,
}
