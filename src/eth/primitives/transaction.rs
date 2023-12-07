use ethers_core::types::Transaction as EthersTransaction;

use crate::derive_newtype_from;

#[derive(Debug)]
pub struct Transaction(EthersTransaction);

// -----------------------------------------------------------------------------
// Other -> Self
// -----------------------------------------------------------------------------
derive_newtype_from!(self = Transaction, other = EthersTransaction);
