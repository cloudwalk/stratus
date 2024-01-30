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
    /// * Address present in the `from` field when performing an `eth_call`.
    pub from: Address,

    /// Operation counterparty address.
    ///
    /// It can be:
    /// * Contract address when performing a function call.
    /// * Destination account address when transfering funds.
    /// * Not specified when deploying a contract.
    pub to: Option<Address>,

    /// TODO: document
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

    /// Block number indicating the point-in-time the EVM state will be used to compute the transaction.
    ///
    /// When not specified, assumes the current state.
    pub point_in_time: StoragePointInTime,
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
impl TryFrom<TransactionInput> for EvmInput {
    type Error = anyhow::Error;

    fn try_from(value: TransactionInput) -> anyhow::Result<Self> {
        Ok(Self {
            from: value.signer,
            to: value.to,
            value: value.value,
            data: value.input,
            nonce: Some(value.nonce),
            point_in_time: StoragePointInTime::Present,
        })
    }
}

impl From<(CallInput, StoragePointInTime)> for EvmInput {
    fn from(value: (CallInput, StoragePointInTime)) -> Self {
        Self {
            from: value.0.from,
            to: value.0.to.map_into(),
            value: value.0.value,
            data: value.0.data,
            nonce: None,
            point_in_time: value.1,
        }
    }
}
