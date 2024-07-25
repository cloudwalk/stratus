//! Auto-generated code.

use crate::eth::primitives::Address;

include!(concat!(env!("OUT_DIR"), "/contracts.rs"));
include!(concat!(env!("OUT_DIR"), "/signatures.rs"));

pub type ContractName = &'static str;

/// Returns the contract name for a given address.
pub fn get_contract_name(address: &Address) -> ContractName {
    match CONTRACTS.get(address.as_bytes()) {
        Some(contract_name) => contract_name,
        None => "unknown",
    }
}
