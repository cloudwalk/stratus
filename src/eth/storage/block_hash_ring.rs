//! Retention of recent block hashes for the EVM `BLOCKHASH` opcode.

use std::num::NonZeroUsize;

use parking_lot::RwLock;

use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::storage::BlockReference;

/// A ring position, empty until a block whose number maps to it is published.
type RingSlot = Option<BlockReference>;

/// Fixed size ring of block hashes, indexed by `number % capacity`.
///
/// Block numbers are dense and published in order, so the ring holds exactly the most recent
/// `capacity` blocks. A general purpose cache cannot promise that: it spreads entries over shards
/// sized independently of each other, and within a shard it evicts the never read entries first,
/// which is precisely what a freshly sealed hash is. Importer-offline depends on the retention
/// being exact, because the permanent storage cannot answer for blocks it has not saved yet.
///
/// Two numbers share a slot only when they are `capacity` apart, so reading an older hash back
/// from the permanent storage cannot displace a block that is still inside the window. Callers are
/// expected to keep the capacity at or above the range they read back, which the default does for
/// the 256 block `BLOCKHASH` window.
pub struct BlockHashRing {
    capacity: NonZeroUsize,
    slots: RwLock<Box<[RingSlot]>>,
}

impl BlockHashRing {
    pub fn new(capacity: NonZeroUsize) -> Self {
        Self {
            capacity,
            slots: RwLock::new(vec![RingSlot::None; capacity.get()].into_boxed_slice()),
        }
    }

    /// The hash of a block never changes, so this overwrites whatever occupied the slot.
    pub fn insert(&self, number: BlockNumber, hash: Hash) {
        let slot = self.slot_of(number);
        self.slots.write()[slot] = Some(BlockReference { number, hash });
    }

    pub fn get(&self, number: BlockNumber) -> Option<Hash> {
        let slot = self.slot_of(number);
        match self.slots.read()[slot] {
            Some(occupant) if occupant.number == number => Some(occupant.hash),
            _ => None,
        }
    }

    pub fn clear(&self) {
        self.slots.write().fill(RingSlot::None);
    }

    fn slot_of(&self, number: BlockNumber) -> usize {
        (number.as_u64() % self.capacity.get() as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_ring(capacity: usize) -> BlockHashRing {
        BlockHashRing::new(NonZeroUsize::new(capacity).unwrap())
    }

    fn block_hash(number: u64) -> Hash {
        let mut bytes = [0_u8; 32];
        bytes[..8].copy_from_slice(&number.to_be_bytes());
        Hash::new(bytes)
    }

    fn publish(ring: &BlockHashRing, numbers: impl IntoIterator<Item = u64>) {
        for number in numbers {
            ring.insert(BlockNumber::from(number), block_hash(number));
        }
    }

    fn read(ring: &BlockHashRing, number: u64) -> Option<Hash> {
        ring.get(BlockNumber::from(number))
    }

    /// The whole point of the ring: a full window is retained, with no entry lost to an eviction
    /// policy or to an unlucky shard.
    #[test]
    fn every_block_in_the_configured_window_is_retained() {
        let ring = new_ring(8);

        publish(&ring, 1..=8);

        for number in 1..=8 {
            assert_eq!(read(&ring, number), Some(block_hash(number)), "block {number}");
        }
    }

    #[test]
    fn only_blocks_that_left_the_window_are_dropped() {
        let ring = new_ring(4);

        publish(&ring, 1..=6);

        assert_eq!(read(&ring, 1), None);
        assert_eq!(read(&ring, 2), None);
        for number in 3..=6 {
            assert_eq!(read(&ring, number), Some(block_hash(number)), "block {number}");
        }
    }

    /// Reading an older hash back from the permanent storage must not cost the window a block,
    /// which is what made the sealed-but-unsaved hashes of importer-offline evictable before.
    #[test]
    fn reading_an_older_hash_back_does_not_displace_the_window() {
        let ring = new_ring(4);
        publish(&ring, 6..=8);

        publish(&ring, [5]);

        for number in 5..=8 {
            assert_eq!(read(&ring, number), Some(block_hash(number)), "block {number}");
        }
    }

    #[test]
    fn clearing_drops_published_block_hashes() {
        let ring = new_ring(4);
        publish(&ring, 1..=4);

        ring.clear();

        assert_eq!(read(&ring, 4), None);
    }
}
