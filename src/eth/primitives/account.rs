//! Account Module
//!
//! The Account module is central to Ethereum's functionality, representing
//! both user wallets and contracts. It encapsulates key aspects of an Ethereum
//! account, such as its unique address, nonce (which tracks the number of
//! transactions sent from the account), current balance, and in the case of
//! smart contracts, their associated bytecode. This module is pivotal for
//! tracking account states and differentiating between standard accounts and
//! contract accounts.

use std::collections::HashSet;

use revm::primitives::AccountInfo as RevmAccountInfo;
use revm::primitives::Address as RevmAddress;

use crate::eth::primitives::parse_bytecode_slots_indexes;
use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::CodeHash;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::SlotAccess;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::Wei;
use crate::ext::not;
use crate::ext::OptionExt;

/// Ethereum account (wallet or contract).
#[derive(Debug, Clone, Default, PartialEq, Eq, fake::Dummy, serde::Deserialize, serde::Serialize)]
pub struct Account {
    /// Immutable address of the account.
    pub address: Address,

    /// Current nonce of the account. Changes every time a transaction is sent.
    pub nonce: Nonce,

    /// Current balance of the account. Changes when a transfer is made or the account pays a fee for executing a transaction.
    pub balance: Wei,

    /// Contract bytecode. Present only if the account is a contract.
    pub bytecode: Option<Bytes>,

    /// Keccak256 Hash of the bytecode. If bytecode is null, then the hash of empty string.
    pub code_hash: CodeHash,

    /// Slots indexes that are accessed statically.
    pub slot_indexes_static_access: Option<Vec<SlotIndex>>,

    /// Slots indexes that are accessed using the mapping hash algorithm.
    pub slot_indexes_mapping_access: Option<Vec<SlotIndex>>,
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
            code_hash: CodeHash::default(),
            slot_indexes_static_access: None,
            slot_indexes_mapping_access: None,
        }
    }

    /// Checks the current account is empty.
    ///
    /// <https://eips.ethereum.org/EIPS/eip-7523#:~:text=An%20empty%20account%20is%20an%20account>
    pub fn is_empty(&self) -> bool {
        self.nonce.is_zero() && self.balance.is_zero() && self.bytecode.is_none()
    }

    /// Checks the current account is a contract.
    pub fn is_contract(&self) -> bool {
        match self.bytecode {
            Some(ref bytecode) => !bytecode.is_empty(),
            None => false,
        }
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
impl From<(RevmAddress, RevmAccountInfo)> for Account {
    fn from(value: (RevmAddress, RevmAccountInfo)) -> Self {
        let (address, info) = value;

        // parse bytecode
        let slot_indexes: HashSet<SlotAccess> = match info.code {
            Some(ref bytecode) if not(bytecode.is_empty()) => parse_bytecode_slots_indexes(bytecode.clone().into()),
            _ => HashSet::new(),
        };

        let mut static_access = Vec::with_capacity(slot_indexes.len());
        let mut mapping_access = Vec::with_capacity(slot_indexes.len());
        for index in slot_indexes {
            match index {
                SlotAccess::Static(index) => static_access.push(index),
                SlotAccess::Mapping(index) => mapping_access.push(index),
                _ => {}
            }
        }

        Self {
            address: address.into(),
            nonce: info.nonce.into(),
            balance: info.balance.into(),
            bytecode: info.code.map_into(),
            code_hash: info.code_hash.into(),
            slot_indexes_static_access: match static_access.is_empty() {
                true => None,
                false => Some(static_access),
            },
            slot_indexes_mapping_access: match mapping_access.is_empty() {
                true => None,
                false => Some(mapping_access),
            },
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
            code_hash: value.code_hash.inner().0.into(),
            code: value.bytecode.map_into(),
        }
    }
}

// -----------------------------------------------------------------------------
// Utilities
// -----------------------------------------------------------------------------

/// Retrieves test accounts.
pub fn test_accounts() -> Vec<Account> {
    use hex_literal::hex;

    [
        hex!("f39fd6e51aad88f6f4ce6ab8827279cfffb92266"),
        hex!("70997970c51812dc3a010c7d01b50e0d17dc79c8"),
        hex!("3c44cdddb6a900fa2b585dd299e03d12fa4293bc"),
        hex!("15d34aaf54267db7d7c367839aaf71a00a2c6a65"),
        hex!("9965507d1a55bcc2695c58ba16fb37d819b0a4dc"),
        hex!("976ea74026e726554db657fa54763abd0c3a0aa9"),
    ]
    .into_iter()
    .map(|address| Account {
        address: address.into(),
        balance: Wei::TEST_BALANCE,
        ..Account::default()
    })
    .collect()
}
