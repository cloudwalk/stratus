//! Historical Value Module
//!
//! Manages historical data, particularly tracking changes of a value over
//! different block numbers. This is crucial for Ethereum's state management,
//! where understanding the state of a variable at different points in the
//! blockchain is necessary. It supports querying past states, providing a
//! history of changes for a particular piece of data.

use std::fmt::Debug;

use nonempty::NonEmpty;

use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::StoragePointInTime;

/// TODO: document
#[derive(Debug)]
pub struct HistoricalValues<T>(NonEmpty<HistoricalValue<T>>)
where
    T: Clone + Debug;

/// TODO: document
#[derive(Debug, derive_new::new)]
pub struct HistoricalValue<T> {
    block_number: BlockNumber,
    value: T,
}

impl<T> HistoricalValues<T>
where
    T: Clone + Debug,
{
    /// Creates a new list of historical values.
    pub fn new(block_number: BlockNumber, value: T) -> Self {
        let value = HistoricalValue::new(block_number, value);
        Self(NonEmpty::new(value))
    }

    /// Adds a new historical value to the list.
    ///
    /// TODO: should we validate that the block number is greater than the last one?
    pub fn push(&mut self, block_number: BlockNumber, value: T) {
        let value = HistoricalValue::new(block_number, value);
        self.0.push(value);
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
}
