//! `Evm` Trait and `EvmInput` Structure
//!
//! Defines a standard interface for Ethereum Virtual Machine (EVM) implementations within the Stratus project,
//! allowing for execution of smart contracts and transactions. `EvmInput` encapsulates all necessary parameters
//! for EVM operations, including sender, receiver, value, data, nonce, and execution context. This abstraction
//! facilitates flexible EVM integrations, enabling the project to adapt to different blockchain environments
//! or requirements while maintaining a consistent execution interface.

use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::CallInput;
use crate::eth::primitives::Execution;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::ExternalTransaction;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::UnixTime;
use crate::eth::primitives::Wei;
use crate::ext::OptionExt;
use crate::if_else;

/// EVM operations.
pub trait Evm: Send + Sync + 'static {
    /// Execute a transaction that deploys a contract or call a contract function.
    fn execute(&mut self, input: EvmInput) -> anyhow::Result<Execution>;
}

/// EVM input data. Usually derived from a transaction or call.
#[derive(Debug, Clone)]
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
    pub point_in_time: StoragePointInTime,
}

impl EvmInput {
    /// Creates from a transaction that was sent directly to Stratus with `eth_sendRawTransaction`.
    pub fn from_eth_transaction(input: TransactionInput) -> Self {
        Self {
            from: input.signer,
            to: input.to,
            value: input.value,
            data: input.input,
            gas_limit: input.gas_limit,
            gas_price: Wei::ZERO, // XXX: use value from input?
            nonce: Some(input.nonce),
            block_number: BlockNumber::ZERO, // TODO: use number of block being mined
            block_timestamp: UnixTime::now(),
            point_in_time: StoragePointInTime::Present,
        }
    }

    /// Creates from a call that was sent directly to Stratus with `eth_call` or `eth_estimateGas`.
    pub fn from_eth_call(input: CallInput, point_in_time: StoragePointInTime) -> Self {
        Self {
            from: input.from.unwrap_or(Address::ZERO),
            to: input.to.map_into(),
            value: input.value,
            data: input.data,
            gas_limit: Gas::MAX,  // XXX: use value from input?
            gas_price: Wei::ZERO, // XXX: use value from input?
            nonce: None,
            block_number: match point_in_time {
                StoragePointInTime::Present => BlockNumber::ZERO, // TODO: use number of block being mined
                StoragePointInTime::Past(number) => number,
            },
            block_timestamp: match point_in_time {
                StoragePointInTime::Present => UnixTime::now(),
                StoragePointInTime::Past(_) => UnixTime::now(), // TODO: use timestamp of the specified block
            },
            point_in_time,
        }
    }

    /// Creates a transaction that was executed in an external blockchain and imported to Stratus.
    pub fn from_external_transaction(block: &ExternalBlock, tx: ExternalTransaction, receipt: &ExternalReceipt) -> Self {
        let gas_limit = if_else!(receipt.is_success(), Gas::MAX, tx.0.gas.into());
        Self {
            from: tx.0.from.into(),
            to: tx.0.to.map_into(),
            value: tx.0.value.into(),
            data: tx.0.input.into(),
            nonce: Some(tx.0.nonce.into()),
            gas_limit,
            gas_price: tx.0.gas_price.expect("gas_price must be set for ExternalTransaction").into(), // XXX: how to handle transactions without gas price?
            point_in_time: StoragePointInTime::Present,
            block_number: block.number(),
            block_timestamp: block.timestamp(),
        }
    }
}
