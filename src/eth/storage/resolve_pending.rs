use parking_lot::RwLockReadGuard;

use crate::eth::storage::ExecutionKind;
use crate::eth::storage::StratusStorage;
use crate::eth::storage::stratus_storage::EntityRead;
use crate::eth::types::Account;
use crate::eth::types::BlockNumber;
use crate::eth::types::PointInTime;
use crate::eth::types::Slot;
use crate::infra::metrics::MetricLabelValue;

/// Prevents construction of [`MinedPointInTime`] outside this module.
/// `Seal` is public (so the enum variants can be pattern-matched) but cannot be constructed
/// externally because its field type is private.
#[derive(Debug)]
pub struct Seal(SealPrivate);

#[derive(Debug)]
struct SealPrivate;

/// A [`PointInTime`] that has been resolved past the pending case.
///
/// `Latest` carries an optional `transient_state_lock` read guard held across the latest read.
///
/// The [`Seal`] field makes both variants impossible to construct outside this module,
/// while still allowing pattern matching externally.
#[derive(Debug, strum::Display)]
pub enum MinedPointInTime<'a> {
    #[strum(to_string = "latest")]
    Latest(Seal, Option<RwLockReadGuard<'a, ()>>),
    #[strum(to_string = "past")]
    Past(Seal, BlockNumber),
}

impl<'a> MinedPointInTime<'a> {
    fn latest(guard: Option<RwLockReadGuard<'a, ()>>) -> Self {
        Self::Latest(Seal(SealPrivate), guard)
    }

    fn past(number: BlockNumber) -> Self {
        Self::Past(Seal(SealPrivate), number)
    }

    /// Extracts the read guard if present, leaving `Mined(None)` in its place.
    fn take_guard(&mut self) -> Option<RwLockReadGuard<'a, ()>> {
        match self {
            Self::Latest(_, guard) => guard.take(),
            Self::Past(_, _) => None,
        }
    }
}

impl From<MinedPointInTime<'_>> for MetricLabelValue {
    fn from(value: MinedPointInTime<'_>) -> Self {
        Self::Some(value.to_string())
    }
}

/// Unlocks the guard fairly when dropped.
impl<'a> Drop for MinedPointInTime<'a> {
    fn drop(&mut self) {
        if let Some(guard) = self.take_guard() {
            RwLockReadGuard::unlock_fair(guard);
        }
    }
}

/// Outcome of resolving pending state for a read.
#[derive(Debug)]
pub(super) enum Resolved<'a, T> {
    /// Found in temporary storage.
    Temp(T),
    /// Nothing pending.
    Miss(MinedPointInTime<'a>),
}

/// Pending-state resolution, generic over the entity being read.
pub(super) trait Resolve: EntityRead {
    fn resolve(s: &StratusStorage, key: Self::Key, kind: ExecutionKind) -> Resolved<'_, Self> {
        if kind.point_in_time() == PointInTime::Pending
            && let Some(value) = Self::read_temp(s, key)
        {
            return Resolved::Temp(value);
        }
        Resolved::Miss(s.resolve_mined_point(kind))
    }
}

impl Resolve for Account {}

impl Resolve for Slot {}

impl StratusStorage {
    fn resolve_call_point(&self, block_number: BlockNumber) -> MinedPointInTime<'_> {
        let guard = self.transient_state_lock.read();
        let mined = self.read_mined_block_number();
        if block_number >= mined {
            MinedPointInTime::latest(Some(guard))
        } else {
            MinedPointInTime::past(block_number)
        }
    }

    /// Determines the mined point-in-time for a read.
    fn resolve_mined_point(&self, kind: ExecutionKind) -> MinedPointInTime<'_> {
        match kind {
            ExecutionKind::RPC(PointInTime::Past(number)) | ExecutionKind::CallPast(number) => MinedPointInTime::past(number),
            ExecutionKind::CallLatest(block_number) => self.resolve_call_point(block_number),
            ExecutionKind::Transaction | ExecutionKind::RPC(_) => MinedPointInTime::latest(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::StratusStorage;
    use super::Resolve;
    use crate::eth::storage::ExecutionKind;
    use crate::eth::types::Address;
    use crate::eth::types::BlockNumber;
    use crate::eth::types::Slot;
    use crate::eth::types::SlotIndex;

    #[test]
    fn mined_full_call_downgrades_to_minedpast_block_not_prev() {
        let storage = StratusStorage::new_test().expect("failed to build test storage");

        let address = Address::ZERO;
        let index = SlotIndex::ZERO;

        // Mined Full call: block_number = 5, mined = 5 → valid (b >= mined).
        let call_block = BlockNumber::from(5u64);
        storage.set_mined_block_number(call_block);

        let kind = ExecutionKind::CallLatest(call_block);

        let resolved = Slot::resolve(&storage, (address, index), kind);
        match resolved {
            super::Resolved::Miss(mut point) => {
                assert!(
                    matches!(point, super::MinedPointInTime::Latest(_, _)),
                    "Full call should read latest while block is the mined tip"
                );
                assert!(point.take_guard().is_some(), "guard should be held for valid latest read");
            }
            other => panic!("expected Miss, got {other:?}"),
        }

        // A newer block is mined mid-call, advancing the mined tip to 6.
        storage.set_mined_block_number(BlockNumber::from(6u64));

        // Stale: b=5 < mined=6. Full → MinedPast(5), NOT MinedPast(4).
        let resolved = Slot::resolve(&storage, (address, index), kind);
        match resolved {
            super::Resolved::Miss(mut point) => {
                assert!(!matches!(point, super::MinedPointInTime::Latest(_, _)), "stale call should not read latest");
                match &point {
                    super::MinedPointInTime::Past(_, number) => {
                        assert_eq!(*number, call_block, "stale Full call should downgrade to MinedPast(block_number), not prev()");
                    }
                    other => panic!("expected Past, got {other:?}"),
                }
                assert!(point.take_guard().is_none(), "no guard for historical read");
            }
            other => panic!("expected Miss, got {other:?}"),
        }
    }
}
