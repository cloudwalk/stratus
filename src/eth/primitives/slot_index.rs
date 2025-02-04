use std::fmt::Display;
use std::io::Read;

use display_json::DebugAsJson;
use ethereum_types::U256;
use alloy_primitives::keccak256;
use fake::Dummy;
use fake::Faker;

use crate::alias::RevmU256;
use crate::gen_newtype_from;

#[derive(DebugAsJson, Clone, Copy, Default, Hash, Eq, PartialEq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct SlotIndex(pub U256);

impl SlotIndex {
    pub const ZERO: SlotIndex = SlotIndex(U256::zero());
    pub const ONE: SlotIndex = SlotIndex(U256::one());

    /// Computes the mapping index of a key.
    pub fn to_mapping_index(&self, key: Vec<u8>) -> SlotIndex {
        // populate self to bytes
        let mut slot_index_bytes = [0u8; 32];
        self.0.to_big_endian(&mut slot_index_bytes);

        // populate key to bytes
        let mut key_bytes = [0u8; 32];
        let _ = key.take(32).read(&mut key_bytes[32usize.saturating_sub(key.len())..32]);

        // populate value to be hashed to bytes
        let mut mapping_index_bytes = [0u8; 64];
        mapping_index_bytes[0..32].copy_from_slice(&key_bytes);
        mapping_index_bytes[32..64].copy_from_slice(&slot_index_bytes);

        let hashed_bytes = keccak256(mapping_index_bytes);
        Self::from(hashed_bytes.0)
    }
}

impl Dummy<Faker> for SlotIndex {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

impl Display for SlotIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#x}", self.0)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

gen_newtype_from!(self = SlotIndex, other = u64, U256, [u8; 32]);

impl From<[u64; 4]> for SlotIndex {
    fn from(value: [u64; 4]) -> Self {
        Self(U256(value))
    }
}

impl From<RevmU256> for SlotIndex {
    fn from(value: RevmU256) -> Self {
        Self(value.to_be_bytes().into())
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use hex_literal::hex;

    use crate::eth::primitives::SlotIndex;

    #[test]
    fn slot_index_to_mapping_index() {
        let address = hex!("3c44cdddb6a900fa2b585dd299e03d12fa4293bc").to_vec();
        let hashed = SlotIndex::ZERO.to_mapping_index(address);
        assert_eq!(hashed.to_string(), "0x215be5d23550ceb1beff54fb579a765903ba2ccc85b6f79bcf9bda4e8cb86034");
    }
}
