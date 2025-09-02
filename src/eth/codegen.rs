//! Auto-generated code.

use std::borrow::Cow;

use crate::eth::primitives::Address;
use crate::infra::metrics;

include!(concat!(env!("OUT_DIR"), "/contracts.rs"));
include!(concat!(env!("OUT_DIR"), "/signatures.rs"));

pub type SoliditySignature = &'static str;

pub type ContractName = &'static str;

/// Returns the contract name to be used in observability tasks.
pub fn contract_name(address: &Option<Address>) -> ContractName {
    let Some(address) = address else { return metrics::LABEL_MISSING };
    match CONTRACTS.get(address.as_slice()) {
        Some(contract_name) => contract_name,
        None => metrics::LABEL_UNKNOWN,
    }
}

/// Returns the function name to be used in observability tasks.
pub fn function_sig(bytes: impl AsRef<[u8]>) -> SoliditySignature {
    match function_sig_opt(bytes) {
        Some(signature) => signature,
        None => metrics::LABEL_UNKNOWN,
    }
}

/// Returns the function name to be used in observability tasks.
pub fn function_sig_opt(bytes: impl AsRef<[u8]>) -> Option<SoliditySignature> {
    let Some(id) = bytes.as_ref().get(..4) else {
        return Some(metrics::LABEL_MISSING);
    };
    SIGNATURES_4_BYTES.get(id).copied()
}

/// Returns the error name or string.
pub fn error_sig_opt(bytes: impl AsRef<[u8]>) -> Option<Cow<'static, str>> {
    bytes
        .as_ref()
        .get(..4)
        .and_then(|id| SIGNATURES_4_BYTES.get(id))
        .copied()
        .map(Cow::Borrowed)
        .or_else(|| {
            bytes.as_ref().get(4..).and_then(|bytes| {
                ethabi::decode(&[ethabi::ParamType::String], bytes)
                    .ok()
                    .and_then(|res| res.first().cloned())
                    .and_then(|token| token.into_string())
                    .map(Cow::Owned)
            })
        })
}
