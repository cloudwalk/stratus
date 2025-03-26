use display_json::DebugAsJson;
use ethereum_types::H256;
use fake::Dummy;
use fake::Faker;
use revm::primitives::FixedBytes;
use revm::primitives::KECCAK_EMPTY;

use crate::gen_newtype_from;

/// Digest of the bytecode of a contract.
/// In the case of an externally-owned account (EOA), bytecode is null
/// and the code hash is fixed as the keccak256 hash of an empty string
#[derive(DebugAsJson, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CodeHash(pub H256);

impl Dummy<Faker> for CodeHash {
    fn dummy_with_rng<R: rand_core::RngCore + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        Self(H256::random_using(rng))
    }
}

impl CodeHash {
    pub fn new(inner: H256) -> Self {
        CodeHash(inner)
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

impl From<FixedBytes<32>> for CodeHash {
    fn from(value: FixedBytes<32>) -> Self {
        CodeHash::new(value.0.into())
    }
}
