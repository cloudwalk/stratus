use ethereum_types::H256;
use ethers_core::utils::keccak256;
use fake::Dummy;
use fake::Faker;
use revm::primitives::FixedBytes;
use revm::primitives::KECCAK_EMPTY;

use crate::eth::primitives::Bytes;
use crate::gen_newtype_from;

/// Digest of the bytecode of a contract.
/// In the case of an externally-owned account (EOA), bytecode is null
/// and the code hash is fixed as the keccak256 hash of an empty string
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CodeHash(H256);

impl Dummy<Faker> for CodeHash {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        Self(H256::random_using(rng))
    }
}

impl CodeHash {
    pub fn new(inner: H256) -> Self {
        CodeHash(inner)
    }

    pub fn from_bytecode(maybe_bytecode: Option<Bytes>) -> Self {
        match maybe_bytecode {
            Some(bytecode) => CodeHash(H256::from_slice(&keccak256(bytecode.as_ref()))),
            None => CodeHash::default(),
        }
    }

    pub fn inner(&self) -> H256 {
        self.0
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = CodeHash, other = [u8; 32]);

impl Default for CodeHash {
    fn default() -> Self {
        CodeHash(KECCAK_EMPTY.0.into())
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> other
// -----------------------------------------------------------------------------
impl AsRef<[u8]> for CodeHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl From<FixedBytes<32>> for CodeHash {
    fn from(value: FixedBytes<32>) -> Self {
        CodeHash::new(value.0.into())
    }
}

impl From<Vec<u8>> for CodeHash {
    fn from(value: Vec<u8>) -> Self {
        let value: &[u8; 32] = value.as_slice().try_into().unwrap();
        CodeHash::new(value.into())
    }
}
