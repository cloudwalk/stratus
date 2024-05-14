//! Alias Module
//!
//! This module defines convenient aliases for various Ethereum-specific
//! signatures and identifiers. It includes types like `Signature4Bytes` for
//! identifying function and error signatures, `Signature32Bytes` for event
//! signatures, and `SoliditySignature` for general Solidity function, error,
//! or event signatures. These aliases simplify code readability and
//! maintenance by providing clear, descriptive types in Ethereum-related
//! operations.

use std::borrow::Cow;

/// Alias for 4 byte signature used to identify functions and errors.
pub type Signature4Bytes = [u8; 4];

/// Alias for 32 byte signature used to identify events.
pub type Signature32Bytes = [u8; 32];

/// Alias for a Solidity function, error or event signature.
pub type SoliditySignature = Cow<'static, str>;
