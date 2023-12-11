use revm::primitives::AccountInfo as RevmAccountInfo;
use revm::primitives::Address as RevmAddress;
use revm::primitives::KECCAK_EMPTY;

use crate::eth::primitives::Address;
use crate::eth::primitives::Amount;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::Nonce;

/// Ethereum account (wallet or contract).
#[derive(Debug, Clone, Default)]
pub struct Account {
    /// Immutable address of the account.
    pub address: Address,

    /// Current nonce of the account. Changes every time a transaction is sent.
    pub nonce: Nonce,

    /// Current balance of the account. Changes when a transfer is made or the account pays a fee for executing a transaction.
    pub balance: Amount,

    /// Contract bytecode. Present only if the account is a contract.
    pub bytecode: Option<Bytes>,
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
impl From<(RevmAddress, RevmAccountInfo)> for Account {
    fn from(value: (RevmAddress, RevmAccountInfo)) -> Self {
        Self {
            address: value.0.into(),
            nonce: value.1.nonce.into(),
            balance: value.1.balance.into(),
            bytecode: value.1.code.map(|b| b.into()),
        }
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<Account> for RevmAccountInfo {
    fn from(value: Account) -> Self {
        Self {
            nonce: value.nonce.into(),
            balance: value.balance.into(),
            code_hash: KECCAK_EMPTY,
            code: value.bytecode.map(|b| b.into()),
        }
    }
}
