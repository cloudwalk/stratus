use revm::primitives::KECCAK_EMPTY;

use crate::evm::entities::Address;
use crate::evm::entities::Bytecode;
use crate::evm::entities::Nonce;

#[derive(Debug, Clone, Default)]
pub struct Account {
    pub address: Address,
    pub nonce: Nonce,
    pub bytecode: Option<Bytecode>,
}

impl From<Account> for revm::primitives::AccountInfo {
    fn from(value: Account) -> Self {
        Self {
            balance: revm::primitives::U256::ZERO,
            nonce: value.nonce.0,
            code_hash: KECCAK_EMPTY,
            code: value.bytecode.map(|b| b.into()),
        }
    }
}
