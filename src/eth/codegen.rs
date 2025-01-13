//! Auto-generated code.

use crate::eth::primitives::Address;
use crate::infra::metrics;

include!(concat!(env!("OUT_DIR"), "/contracts.rs"));
include!(concat!(env!("OUT_DIR"), "/signatures.rs"));

pub type SoliditySignature = &'static str;

pub type ContractName = &'static str;

/// Returns the contract name to be used in observability tasks.
pub fn contract_name_for_o11y(address: &Option<Address>) -> ContractName {
    let Some(address) = address else { return metrics::LABEL_MISSING };
    match CONTRACTS.get(address.as_bytes()) {
        Some(contract_name) => contract_name,
        None => metrics::LABEL_UNKNOWN,
    }
}

/// Returns the function name to be used in observability tasks.
pub fn function_sig_for_o11y(bytes: impl AsRef<[u8]>) -> SoliditySignature {
    match function_sig_for_o11y_opt(bytes) {
        Some(signature) => signature,
        None => metrics::LABEL_UNKNOWN,
    }
}

/// Returns the function name to be used in observability tasks.
pub fn function_sig_for_o11y_opt(bytes: impl AsRef<[u8]>) -> Option<SoliditySignature> {
    let Some(id) = bytes.as_ref().get(..4) else {
        return Some(metrics::LABEL_MISSING);
    };
    SIGNATURES_4_BYTES.get(id).copied()
}
