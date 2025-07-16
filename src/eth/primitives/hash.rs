use std::fmt::Display;
use std::str::FromStr;

use alloy_primitives::B256;
use anyhow::anyhow;
use display_json::DebugAsJson;
use fake::Dummy;
use fake::Faker;

#[derive(DebugAsJson, Clone, Copy, Default, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Hash(pub B256);

impl Hash {
    pub const ZERO: Hash = Hash(B256::ZERO);

    /// Creates a hash from the given bytes.
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(B256::from(bytes))
    }
}

impl Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", const_hex::encode_prefixed(self.0))
    }
}

impl Dummy<Faker> for Hash {
    fn dummy_with_rng<R: rand::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        B256::random_with(rng).into()
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

impl FromStr for Hash {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        match B256::from_str(s) {
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
        Self(value)
    }
}

impl From<[u8; 32]> for Hash {
    fn from(value: B256) -> Self {
        Self(B256::from(value))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

impl From<Hash> for B256 {
    fn from(value: Hash) -> Self {
        value.0
    }
}
