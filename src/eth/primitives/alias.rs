/// Alias for 4 byte signature used to identify functions and errors.
pub type Signature4Bytes = [u8; 4];

/// Alias for 32 byte signature used to identify events.
pub type Signature32Bytes = [u8; 32];

/// Alias for a Solidity function, error or event signature.
pub type SoliditySignature = &'static str;
