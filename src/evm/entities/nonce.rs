use std::fmt::Display;

#[derive(Debug, Clone, Default, derive_more::From)]
pub struct Nonce(pub u64);

impl Display for Nonce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
