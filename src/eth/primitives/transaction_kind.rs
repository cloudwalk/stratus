#![allow(clippy::large_enum_variant)]

use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::ExternalTransaction;
use crate::eth::primitives::TransactionInput;

#[derive(Debug, Clone, derive_new::new)]
pub enum TransactionKind {
    /// Transaction that was sent directly to Stratus.
    Local(TransactionInput),

    /// Transaction that imported from external source.
    External(ExternalTransaction, ExternalReceipt),
}
