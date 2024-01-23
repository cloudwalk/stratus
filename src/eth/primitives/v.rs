use ethereum_types::U64;

// Type representing `v` variable
// from the ECDSA signature 
pub struct V(U64);

impl From<V> for U64 {
    fn from(value: V) -> Self {
        value.0
    }
}
