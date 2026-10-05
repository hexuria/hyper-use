//! A bounded, in-memory ring of recent snapshots.
//!
//! Ids are assigned by [`SnapshotRing::push`] in strictly increasing order and
//! are never reused, even after the entry is evicted. Nothing is written to
//! disk. This is not crash recovery and not a persistence layer.

use std::collections::VecDeque;
use std::fmt;

/// Identity of one stored snapshot. Monotonic within one ring.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SnapshotId(u64);

impl SnapshotId {
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for SnapshotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum HistoryError {
    /// The id was issued but has been evicted. `oldest` is the oldest id kept.
    Evicted { id: SnapshotId, oldest: SnapshotId },
    /// The id was never issued by this ring.
    Unknown(SnapshotId),
}

impl fmt::Display for HistoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Evicted { id, oldest } => {
                write!(f, "snapshot {id} was evicted; oldest kept is {oldest}")
            }
            Self::Unknown(id) => write!(f, "snapshot {id} was never recorded"),
        }
    }
}

impl std::error::Error for HistoryError {}

#[derive(Clone, Debug)]
pub struct SnapshotRing<T> {
    cap: usize,
    next: u64,
    entries: VecDeque<(SnapshotId, T)>,
}

impl<T> SnapshotRing<T> {
    pub const DEFAULT_CAP: usize = 16;

    /// A ring that keeps at most `cap` entries. A `cap` of zero is raised to one.
    pub fn with_capacity(cap: usize) -> Self {
        let cap = cap.max(1);
        Self {
            cap,
            next: 1,
            entries: VecDeque::with_capacity(cap),
        }
    }

    pub fn capacity(&self) -> usize {
        self.cap
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Store `value` and return its new id. Evicts the oldest entry when full.
    pub fn push(&mut self, value: T) -> SnapshotId {
        let id = SnapshotId(self.next);
        self.next += 1;
        if self.entries.len() == self.cap {
            self.entries.pop_front();
        }
        self.entries.push_back((id, value));
        id
    }

    pub fn get(&self, id: SnapshotId) -> Result<&T, HistoryError> {
        if id.0 == 0 || id.0 >= self.next {
            return Err(HistoryError::Unknown(id));
        }
        match self.entries.front() {
            Some((oldest, _)) if id < *oldest => Err(HistoryError::Evicted {
                id,
                oldest: *oldest,
            }),
            Some((oldest, _)) => {
                let offset = usize::try_from(id.0 - oldest.0).expect("offset fits in usize");
                Ok(&self.entries[offset].1)
            }
            None => Err(HistoryError::Unknown(id)),
        }
    }

    pub fn latest(&self) -> Option<(SnapshotId, &T)> {
        self.entries.back().map(|(id, value)| (*id, value))
    }

    /// Entries from newest to oldest.
    pub fn iter_recent(&self) -> impl Iterator<Item = (SnapshotId, &T)> {
        self.entries.iter().rev().map(|(id, value)| (*id, value))
    }
}

impl<T> Default for SnapshotRing<T> {
    fn default() -> Self {
        Self::with_capacity(Self::DEFAULT_CAP)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn ring_evicts_oldest_and_ids_never_repeat() {
        let mut ring = SnapshotRing::with_capacity(2);
        let a = ring.push("a");
        let b = ring.push("b");
        let c = ring.push("c");
        assert_eq!((a.get(), b.get(), c.get()), (1, 2, 3));
        assert_eq!(ring.len(), 2);
        assert_eq!(
            ring.get(a).unwrap_err(),
            HistoryError::Evicted { id: a, oldest: b }
        );
        assert_eq!(
            ring.get(a).unwrap_err().to_string(),
            "snapshot 1 was evicted; oldest kept is 2"
        );
        assert_eq!(ring.get(b), Ok(&"b"));
        assert_eq!(ring.get(c), Ok(&"c"));
        assert_eq!(
            ring.get(SnapshotId::new(9)).unwrap_err(),
            HistoryError::Unknown(SnapshotId::new(9))
        );
        assert_eq!(
            ring.get(SnapshotId::new(0)).unwrap_err(),
            HistoryError::Unknown(SnapshotId::new(0))
        );
        assert_eq!(ring.latest(), Some((c, &"c")));
        let recent: Vec<_> = ring.iter_recent().map(|(id, _)| id.get()).collect();
        assert_eq!(recent, [3, 2]);
        assert_eq!(SnapshotRing::<u8>::with_capacity(0).capacity(), 1);
        assert_eq!(SnapshotRing::<u8>::default().capacity(), 16);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(16))]
        #[test]
        fn snapshot_ids_are_strictly_increasing(cap in 1usize..6, pushes in 1usize..40) {
            let mut ring = SnapshotRing::with_capacity(cap);
            let mut last = 0;
            for value in 0..pushes {
                let id = ring.push(value);
                prop_assert!(id.get() > last);
                last = id.get();
                prop_assert_eq!(ring.get(id), Ok(&value));
                prop_assert!(ring.len() <= cap);
            }
        }
    }
}
