//! Call Input Module
//!
//! This module defines the structure for Ethereum transaction call inputs,
//! encompassing essential elements like the sender's address (`from`), the
//! recipient's address (`to`), the amount of Ether to transfer (`value`), and
//! the transaction data payload (`data`). It is crucial in constructing and
//! interpreting transaction calls, especially for smart contract interactions.

use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::Wei;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CallInput {
    pub from: Address,
    pub to: Option<Address>,

    #[serde(default)]
    pub value: Wei,

    #[serde(default)]
    pub data: Bytes,
}
