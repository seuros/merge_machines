//! A counter that only goes up, held as one count per replica.

use crate::{CapacityError, Merge, ReplicaId, free_slot};

/// One replica's contribution to a [`GCounter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// The replica that owns this count.
    pub replica: ReplicaId,
    /// How much that replica has added.
    pub count: u64,
}

/// A counter that only grows, counted separately by each replica.
///
/// ```
/// use merge_machines::{GCounter, Merge};
///
/// let mut left = GCounter::<4>::new();
/// let mut right = GCounter::<4>::new();
///
/// assert_eq!(left.increment(1, 3), Ok(()));
/// assert_eq!(right.increment(2, 7), Ok(()));
///
/// assert_eq!(left.merge(&right), Ok(()));
/// assert_eq!(left.value(), 10);
///
/// // Merging the same state again adds nothing.
/// assert_eq!(left.merge(&right), Ok(()));
/// assert_eq!(left.value(), 10);
/// ```
#[derive(Debug, Clone, Copy, Eq)]
pub struct GCounter<const R: usize> {
    slots: [Option<Entry>; R],
}

impl<const R: usize> GCounter<R> {
    #[must_use]
    pub const fn new() -> Self {
        Self { slots: [None; R] }
    }

    /// Adds `by` to `replica`'s own count, saturating at [`u64::MAX`].
    ///
    /// # Errors
    ///
    /// Returns [`CapacityError`] when `replica` has no slot yet and all `R` are
    /// taken.
    pub fn increment(&mut self, replica: ReplicaId, by: u64) -> Result<(), CapacityError> {
        let entry = self.entry_mut(replica).ok_or(CapacityError)?;
        entry.count = entry.count.saturating_add(by);
        Ok(())
    }

    /// What `replica` has added, zero if it has no slot here.
    #[must_use]
    pub fn count(&self, replica: ReplicaId) -> u64 {
        self.slots
            .iter()
            .flatten()
            .find(|entry| entry.replica == replica)
            .map_or(0, |entry| entry.count)
    }

    /// Every replica's count, saturating at [`u64::MAX`].
    #[must_use]
    pub fn value(&self) -> u64 {
        self.slots
            .iter()
            .flatten()
            .fold(0, |total, entry| total.saturating_add(entry.count))
    }

    /// The per-replica counts, in no particular order.
    pub fn entries(&self) -> impl Iterator<Item = Entry> + '_ {
        self.slots.iter().flatten().copied()
    }

    /// Raises `replica`'s count to `count` if that is higher.
    fn raise(&mut self, replica: ReplicaId, count: u64) -> Result<(), CapacityError> {
        let entry = self.entry_mut(replica).ok_or(CapacityError)?;
        if count > entry.count {
            entry.count = count;
        }
        Ok(())
    }

    fn entry_mut(&mut self, replica: ReplicaId) -> Option<&mut Entry> {
        let held = self
            .slots
            .iter()
            .position(|slot| matches!(slot, Some(entry) if entry.replica == replica));
        if let Some(index) = held {
            return self.slots[index].as_mut();
        }

        let free = free_slot(&self.slots)?;
        self.slots[free] = Some(Entry { replica, count: 0 });
        self.slots[free].as_mut()
    }
}

impl<const R: usize> Merge for GCounter<R> {
    type Error = CapacityError;

    fn merge(&mut self, other: &Self) -> Result<(), Self::Error> {
        for entry in other.entries() {
            self.raise(entry.replica, entry.count)?;
        }
        Ok(())
    }
}

/// Two counters are equal when every replica's count is, whatever order the
/// slots were filled in.
impl<const R: usize> PartialEq for GCounter<R> {
    fn eq(&self, other: &Self) -> bool {
        self.entries()
            .all(|entry| other.count(entry.replica) == entry.count)
            && other
                .entries()
                .all(|entry| self.count(entry.replica) == entry.count)
    }
}

impl<const R: usize> Default for GCounter<R> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
