use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::CallInput;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::StoragerPointInTime;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionInput;
use crate::eth::EthError;

/// EVM operations.
pub trait Evm: Send + Sync + 'static {
    /// Execute a transaction that deploys a contract or call a contract function.
    fn execute(&mut self, input: EvmInput) -> Result<TransactionExecution, EthError>;
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
    pub point_in_time: StoragerPointInTime,
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
impl TryFrom<TransactionInput> for EvmInput {
    type Error = EthError;

    fn try_from(value: TransactionInput) -> Result<Self, Self::Error> {
        Ok(Self {
            from: value.signer()?,
            to: value.to,
            data: value.input,
            nonce: Some(value.nonce),
            point_in_time: StoragerPointInTime::Present,
        })
    }
}

impl From<(CallInput, StoragerPointInTime)> for EvmInput {
    fn from(value: (CallInput, StoragerPointInTime)) -> Self {
        Self {
            from: value.0.from,
            to: Some(value.0.to),
            data: value.0.data,
            nonce: None,
            point_in_time: value.1,
        }
    }
}
