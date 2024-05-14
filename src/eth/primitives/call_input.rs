//! Call Input Module
//!
//! This module defines the structure for Ethereum transaction call inputs,
//! encompassing essential elements like the sender's address (`from`), the
//! recipient's address (`to`), the amount of Ether to transfer (`value`), and
//! the transaction data payload (`data`). It is crucial in constructing and
//! interpreting transaction calls, especially for smart contract interactions.

use super::Signature;
use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::SoliditySignature;
use crate::eth::primitives::Wei;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CallInput {
    pub from: Option<Address>,
    pub to: Option<Address>,

    #[serde(default)]
    pub value: Wei,

    #[serde(default)]
    pub data: Bytes,
}

impl CallInput {
    pub fn extract_function(&self) -> Option<SoliditySignature> {
        let sig = Signature::Function(self.data.get(..4)?.try_into().ok()?);
        Some(sig.extract())
    }
}
