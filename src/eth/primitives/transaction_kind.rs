#![allow(clippy::large_enum_variant)]

use display_json::DebugAsJson;

use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::ExternalTransaction;
use crate::eth::primitives::TransactionInput;

#[derive(DebugAsJson, Clone, derive_new::new, serde::Serialize)]
pub enum TransactionKind {
    /// Transaction that was sent directly to Stratus.
    Local(TransactionInput),

    /// Transaction that imported from external source.
    External(ExternalTransaction, ExternalReceipt),
}
