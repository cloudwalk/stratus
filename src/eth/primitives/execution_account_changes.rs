use display_json::DebugAsJson;

use crate::alias::RevmBytecode;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::Wei;

/// Changes that happened to an account during a transaction.
#[derive(DebugAsJson, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize, Default)]
pub struct ExecutionAccountChanges {
    pub nonce: Option<Nonce>,
    pub balance: Option<Wei>,
    #[dummy(default)]
    pub bytecode: Option<Option<RevmBytecode>>,
}

impl ExecutionAccountChanges {
    /// Updates an existing account state with changes that happened during the transaction.
    pub fn apply_modifications(&mut self, modified_account: Account) {
        self.nonce = Some(modified_account.nonce);
        self.balance = Some(modified_account.balance);
    }

    /// Checks if account nonce, balance or bytecode were modified.
    pub fn is_account_modified(&self) -> bool {
        self.nonce.is_some() || self.balance.is_some() || self.bytecode.is_some()
    }

    pub fn to_account(self, address: Address) -> Account {
        Account {
            address,
            nonce: self.nonce.unwrap_or_default(),
            balance: self.balance.unwrap_or_default(),
            bytecode: self.bytecode.unwrap_or_default(),
        }
    }
}

impl<T> From<T> for ExecutionAccountChanges
where
    T: Into<Account>,
{
    fn from(value: T) -> Self {
        let account: Account = value.into();
        Self {
            nonce: Some(account.nonce),
            balance: Some(account.balance),
            // bytecode
            bytecode: Some(account.bytecode),
        }
    }
}
