use std::fmt::Debug;

use itertools::Itertools;
use nonempty::NonEmpty;

use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::StoragePointInTime;

#[derive(Debug)]
pub struct InMemoryHistory<T>(NonEmpty<InMemoryHistoryValue<T>>)
where
    T: Clone + Debug;

#[derive(Clone, Debug, derive_new::new)]
pub struct InMemoryHistoryValue<T> {
    block_number: BlockNumber,
    value: T,
}

impl<T> InMemoryHistory<T>
where
    T: Clone + Debug,
{
    /// Creates a new list of historical values.
    pub fn new_at_zero(value: T) -> Self {
        Self::new(BlockNumber::ZERO, value)
    }

    /// Creates a new list of historical values.
    pub fn new(block_number: BlockNumber, value: T) -> Self {
        let value = InMemoryHistoryValue::new(block_number, value);
        Self(NonEmpty::new(value))
    }

    /// Adds a new historical value to the list.
    pub fn push(&mut self, block_number: BlockNumber, value: T) {
        let value = InMemoryHistoryValue::new(block_number, value);
        self.0.push(value);
    }

    /// Resets changes to the specified block number.
    pub fn reset_at(&self, block_number: BlockNumber) -> Option<InMemoryHistory<T>> {
        let history = self.0.iter().filter(|x| x.block_number <= block_number).cloned().collect_vec();
        if history.is_empty() {
            None
        } else {
            Some(Self(NonEmpty::from_vec(history).unwrap()))
        }
    }

    /// Returns the value at the given point in time.
    pub fn get_at_point(&self, point_in_time: &StoragePointInTime) -> Option<T> {
        match point_in_time {
            StoragePointInTime::Present => Some(self.get_current()),
            StoragePointInTime::Past(block_number) => self.get_at_block(block_number),
        }
    }

    /// Returns the most recent value before or at the given block number.
    pub fn get_at_block(&self, block_number: &BlockNumber) -> Option<T> {
        self.0.iter().take_while(|x| x.block_number <= *block_number).map(|x| &x.value).last().cloned()
    }

    /// Returns the most recent value.
    pub fn get_current(&self) -> T {
        self.0.last().value.clone()
    }

    /// Returns the most recent value as reference.
    pub fn get_current_ref(&self) -> &T {
        &self.0.last().value
    }
}
