//! `Evm` Trait and `EvmInput` Structure
//!
//! Defines a standard interface for Ethereum Virtual Machine (EVM) implementations within the Stratus project,
//! allowing for execution of smart contracts and transactions. `EvmInput` encapsulates all necessary parameters
//! for EVM operations, including sender, receiver, value, data, nonce, and execution context. This abstraction
//! facilitates flexible EVM integrations, enabling the project to adapt to different blockchain environments
//! or requirements while maintaining a consistent execution interface.

use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::CallInput;
use crate::eth::primitives::Execution;
use crate::eth::primitives::ExternalTransaction;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::Wei;
use crate::ext::OptionExt;

/// EVM operations.
pub trait Evm: Send + Sync + 'static {
    /// Execute a transaction that deploys a contract or call a contract function.
    fn execute(&mut self, input: EvmInput) -> anyhow::Result<Execution>;
}

/// EVM input data. Usually derived from a transaction or call.
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

    /// Block number indicating the point-in-time the EVM state will be used to compute the transaction.
    ///
    /// When not specified, assumes the current state.
    pub point_in_time: StoragePointInTime,
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
impl From<TransactionInput> for EvmInput {
    fn from(value: TransactionInput) -> Self {
        Self {
            from: value.signer,
            to: value.to,
            value: value.value,
            data: value.input,
            gas_limit: Gas::MAX,  // XXX: use value from input?
            gas_price: Wei::ZERO, // XXX: use value from input?
            nonce: Some(value.nonce),
            point_in_time: StoragePointInTime::Present,
        }
    }
}

impl From<ExternalTransaction> for EvmInput {
    fn from(value: ExternalTransaction) -> Self {
        let gas_limit = value.gas_limit();
        Self {
            from: value.0.from.into(),
            to: value.0.to.map_into(),
            value: value.0.value.into(),
            data: value.0.input.into(),
            nonce: Some(value.0.nonce.into()),
            gas_limit,
            gas_price: value.0.gas_price.expect("gas_price must be set for ExternalTransaction").into(), // XXX: how to handle transactions without gas price?
            point_in_time: StoragePointInTime::Present,
        }
    }
}

impl From<(CallInput, StoragePointInTime)> for EvmInput {
    fn from(value: (CallInput, StoragePointInTime)) -> Self {
        Self {
            from: value.0.from.unwrap_or(Address::ZERO),
            to: value.0.to.map_into(),
            value: value.0.value,
            data: value.0.data,
            gas_limit: Gas::MAX,  // XXX: use value from input?
            gas_price: Wei::ZERO, // XXX: use value from input?
            nonce: None,
            point_in_time: value.1,
        }
    }
}
