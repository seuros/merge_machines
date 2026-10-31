//! A counter that goes both ways, from two that only go up.

use crate::{CapacityError, GCounter, Merge, ReplicaId};

/// A counter replicas can both raise and lower.
///
/// ```
/// use merge_machines::{Merge, PNCounter};
///
/// let mut left = PNCounter::<4>::new();
/// let mut right = PNCounter::<4>::new();
///
/// assert_eq!(left.increment(1, 10), Ok(()));
/// assert_eq!(right.decrement(2, 4), Ok(()));
///
/// assert_eq!(left.merge(&right), Ok(()));
/// assert_eq!(left.value(), 6);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PNCounter<const R: usize> {
    increments: GCounter<R>,
    decrements: GCounter<R>,
}

impl<const R: usize> PNCounter<R> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            increments: GCounter::new(),
            decrements: GCounter::new(),
        }
    }

    /// # Errors
    ///
    /// Returns [`CapacityError`] when `replica` has no slot yet and all `R` are
    /// taken.
    pub fn increment(&mut self, replica: ReplicaId, by: u64) -> Result<(), CapacityError> {
        self.increments.increment(replica, by)
    }

    /// # Errors
    ///
    /// Returns [`CapacityError`] when `replica` has no slot yet and all `R` are
    /// taken.
    pub fn decrement(&mut self, replica: ReplicaId, by: u64) -> Result<(), CapacityError> {
        self.decrements.increment(replica, by)
    }

    /// Everything added, minus everything taken away.
    #[must_use]
    pub fn value(&self) -> i128 {
        i128::from(self.increments.value()) - i128::from(self.decrements.value())
    }

    /// Everything added, by every replica.
    #[must_use]
    pub fn added(&self) -> u64 {
        self.increments.value()
    }

    /// Everything taken away, by every replica.
    #[must_use]
    pub fn removed(&self) -> u64 {
        self.decrements.value()
    }
}

impl<const R: usize> Merge for PNCounter<R> {
    type Error = CapacityError;

    fn merge(&mut self, other: &Self) -> Result<(), Self::Error> {
        self.increments.merge(&other.increments)?;
        self.decrements.merge(&other.decrements)
    }
}

impl<const R: usize> Default for PNCounter<R> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
