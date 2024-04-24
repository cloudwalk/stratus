//! Account Module
//!
//! The Account module is central to Ethereum's functionality, representing
//! both user wallets and contracts. It encapsulates key aspects of an Ethereum
//! account, such as its unique address, nonce (which tracks the number of
//! transactions sent from the account), current balance, and in the case of
//! smart contracts, their associated bytecode. This module is pivotal for
//! tracking account states and differentiating between standard accounts and
//! contract accounts.

use itertools::Itertools;
use revm::primitives::AccountInfo as RevmAccountInfo;
use revm::primitives::Address as RevmAddress;

use super::slot::SlotIndexes;
use crate::eth::evm::EvmInputSlotKeys;
use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::CodeHash;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::SlotAccess;
use crate::eth::primitives::Wei;
use crate::ext::OptionExt;

/// Ethereum account (wallet or contract).
///
/// TODO: group bytecode, code_hash, static_slot_indexes and mapping_slot_indexes into a single bytecode struct.
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
    pub static_slot_indexes: Option<SlotIndexes>,

    /// Slots indexes that are accessed using the mapping hash algorithm.
    pub mapping_slot_indexes: Option<SlotIndexes>,
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
            static_slot_indexes: None,
            mapping_slot_indexes: None,
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

    /// Compute slot indexes to be accessed for a give input.
    pub fn slot_indexes(&self, input_keys: EvmInputSlotKeys) -> SlotIndexes {
        let mut slot_indexes = SlotIndexes::new();

        // calculate static indexes
        if let Some(ref indexes) = self.static_slot_indexes {
            slot_indexes.extend(indexes.0.clone());
        }

        // calculate mapping indexes
        if let Some(ref indexes) = self.mapping_slot_indexes {
            for (base_slot_index, input_key) in indexes.iter().cartesian_product(input_keys.into_iter()) {
                let mapping_slot_index = base_slot_index.to_mapping_index(input_key);
                slot_indexes.insert(mapping_slot_index);
            }
        }

        slot_indexes
    }

    /// Adds a bytecode slot index according to its type.
    pub fn add_bytecode_slot_index(&mut self, index: SlotAccess) {
        match index {
            SlotAccess::Static(index) => {
                self.static_slot_indexes.get_or_insert_with(SlotIndexes::new).insert(index);
            }
            SlotAccess::Mapping(index) => {
                self.mapping_slot_indexes.get_or_insert_with(SlotIndexes::new).insert(index);
            }
            _ => {}
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
            bytecode: info.code.map_into(),
            code_hash: info.code_hash.into(),
            static_slot_indexes: None,
            mapping_slot_indexes: None,
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
