use revm::primitives::KECCAK_EMPTY;

use crate::eth::primitives::Address;
use crate::eth::primitives::Bytecode;
use crate::eth::primitives::Nonce;

#[derive(Debug, Clone, Default)]
pub struct Account {
    pub address: Address,
    pub nonce: Nonce,
    pub bytecode: Option<Bytecode>,
}

// -----------------------------------------------------------------------------
// Self -> Other
// -----------------------------------------------------------------------------
impl From<Account> for revm::primitives::AccountInfo {
    fn from(value: Account) -> Self {
        Self {
            balance: revm::primitives::U256::ZERO,
            nonce: value.nonce.into(),
            code_hash: KECCAK_EMPTY,
            code: value.bytecode.map(|b| b.into()),
        }
    }
}
