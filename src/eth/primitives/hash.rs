use std::fmt::Display;
use std::str::FromStr;

use ethereum_types::H256;
use fake::Dummy;
use fake::Faker;

use crate::eth::EthError;
use crate::gen_newtype_from;

#[derive(Debug, Clone, Default, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Hash(H256);

impl Hash {
    /// Creates a hash from the given bytes.
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(H256(bytes))
    }

    /// Creates a new random hash.
    pub fn new_random() -> Self {
        Self(H256::random())
    }
}

impl Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", const_hex::encode_prefixed(self.0))
    }
}

impl Dummy<Faker> for Hash {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        H256::random_using(rng).into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = Hash, other = H256, [u8; 32]);

impl FromStr for Hash {
    type Err = EthError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match H256::from_str(s) {
            Ok(parsed) => Ok(Self(parsed)),
            Err(e) => {
                tracing::warn!(reason = ?e, value = %s, "failed to parse hash");
                Err(EthError::new_invalid_field("hash", s.to_owned()))
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl AsRef<[u8]> for Hash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl From<Hash> for H256 {
    fn from(value: Hash) -> Self {
        value.0
    }
}
