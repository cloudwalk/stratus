use std::fmt::Display;
use std::str::FromStr;

use alloy_primitives::B256;
use anyhow::anyhow;
use display_json::DebugAsJson;
use ethereum_types::H256;
use fake::Dummy;
use fake::Faker;

use crate::gen_newtype_from;

#[derive(DebugAsJson, Clone, Copy, Default, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Hash(pub H256);

impl Hash {
    pub const ZERO: Hash = Hash(H256::zero());

    /// Creates a hash from the given bytes.
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(H256(bytes))
    }
}

impl Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", const_hex::encode_prefixed(self.0))
    }
}

impl Dummy<Faker> for Hash {
    fn dummy_with_rng<R: rand_core::RngCore + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        H256::random_using(rng).into()
    }
}

impl AsRef<[u8]> for Hash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = Hash, other = H256, [u8; 32]);

impl FromStr for Hash {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        match H256::from_str(s) {
            Ok(parsed) => Ok(Self(parsed)),
            Err(e) => {
                tracing::warn!(reason = ?e, value = %s, "failed to parse hash");
                Err(anyhow!("Failed to parse field 'hash' with value '{}'", s.to_owned()))
            }
        }
    }
}

impl From<B256> for Hash {
    fn from(value: B256) -> Self {
        Self(H256(value.0))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

impl From<Hash> for H256 {
    fn from(value: Hash) -> Self {
        value.0
    }
}

impl From<Hash> for B256 {
    fn from(value: Hash) -> Self {
        Self::from(value.0.to_fixed_bytes())
    }
}
