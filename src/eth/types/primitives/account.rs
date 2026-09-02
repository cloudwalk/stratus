use display_json::DebugAsJson;
use revm::primitives::KECCAK_EMPTY;
use revm::state::Bytecode;

use crate::alias::RevmAccountInfo;
use crate::alias::RevmAddress;
use crate::eth::types::Address;
use crate::eth::types::Nonce;
use crate::eth::types::Wei;

/// Ethereum account (wallet or contract).
///
/// TODO: group bytecode, code_hash, static_slot_indexes and mapping_slot_indexes into a single bytecode struct.
#[derive(DebugAsJson, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(serde::Deserialize, fake::Dummy))]
pub struct Account {
    /// Immutable address of the account.
    pub address: Address,

    /// Current nonce of the account. Changes every time a transaction is sent.
    pub nonce: Nonce,

    /// Current balance of the account. Changes when a transfer is made or the account pays a fee for executing a transaction.
    pub balance: Wei,

    /// Contract bytecode. Present only if the account is a contract.
    #[cfg_attr(test, dummy(default))]
    pub bytecode: Option<Bytecode>,
}

impl Account {
    /// Creates a new empty account.
    pub fn new_empty(address: Address) -> Self {
        Self::new_with_balance(address, Wei::ZERO)
    }

    /// Creates a new account with initial balance.
    pub fn new_with_balance(address: Address, balance: Wei) -> Self {
        Self {
            address,
            nonce: Nonce::ZERO,
            balance,
            bytecode: None,
        }
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
impl From<(RevmAddress, RevmAccountInfo)> for Account {
    fn from(value: (RevmAddress, RevmAccountInfo)) -> Self {
        let (address, info) = value;

        Self {
            address: address.into(),
            nonce: info.nonce.into(),
            balance: info.balance.into(),
            bytecode: info.code,
        }
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

impl From<Account> for RevmAccountInfo {
    fn from(value: Account) -> Self {
        Self {
            account_id: None,
            nonce: value.nonce.into(),
            balance: value.balance.into(),
            // The code hash is the contract's identity: revm 43 propagates it into the frame's
            // precomputed bytecode hash, and consumers dispatch on it (revmc JIT/AOT code cache,
            // EXTCODEHASH). It must be the real keccak of the code, not the empty hash.
            code_hash: value.bytecode.as_ref().map_or(KECCAK_EMPTY, Bytecode::hash_slow),
            code: value.bytecode,
        }
    }
}

// -----------------------------------------------------------------------------
// Utilities
// -----------------------------------------------------------------------------

/// Accounts to be used only in development-mode.
#[cfg(any(test, feature = "dev"))]
pub fn test_accounts() -> Vec<Account> {
    use hex_literal::hex;
    [
        hex!("f39fd6e51aad88f6f4ce6ab8827279cfffb92266"), // ALICE
        hex!("70997970c51812dc3a010c7d01b50e0d17dc79c8"), // BOB
        hex!("3c44cdddb6a900fa2b585dd299e03d12fa4293bc"), // CHARLIE
        hex!("15d34aaf54267db7d7c367839aaf71a00a2c6a65"), // DAVE
        hex!("9965507d1a55bcc2695c58ba16fb37d819b0a4dc"), // EVE
        hex!("976ea74026e726554db657fa54763abd0c3a0aa9"), // FERDIE
        hex!("e45b176cad7090a5cf70b69a73b6def9296ba6a2"), // ?
    ]
    .into_iter()
    .map(|address| Account {
        address: address.into(),
        balance: Wei::TEST_BALANCE,
        ..Account::default()
    })
    .collect()
}
