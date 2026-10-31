//! Last writer wins, with the replica id as the tie-break.

use crate::{Merge, ReplicaId};
use core::convert::Infallible;

/// One value, held by whichever write was stamped last.
///
/// ```
/// use merge_machines::{LwwRegister, Merge};
///
/// let mut left = LwwRegister::new(10_u32, 4_u64, 1);
/// let right = LwwRegister::new(20, 9, 2);
///
/// assert_eq!(left.merge(&right), Ok(()));
/// assert_eq!(*left.value(), 20);
///
/// // A stamp from before the current one changes nothing.
/// left.set(30, 1, 1);
/// assert_eq!(*left.value(), 20);
///
/// // Equal stamps go to the higher replica id, from either side.
/// let mut low = LwwRegister::new(1_u32, 5_u64, 1);
/// let high = LwwRegister::new(2, 5, 2);
/// assert_eq!(low.merge(&high), Ok(()));
/// assert_eq!(*low.value(), 2);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LwwRegister<T, S> {
    value: T,
    stamp: S,
    replica: ReplicaId,
}

impl<T: Clone, S: Ord + Copy> LwwRegister<T, S> {
    #[must_use]
    pub const fn new(value: T, stamp: S, replica: ReplicaId) -> Self {
        Self {
            value,
            stamp,
            replica,
        }
    }

    /// Writes `value` if `(stamp, replica)` beats the write already held.
    pub fn set(&mut self, value: T, stamp: S, replica: ReplicaId) {
        if (stamp, replica) > (self.stamp, self.replica) {
            self.value = value;
            self.stamp = stamp;
            self.replica = replica;
        }
    }

    #[must_use]
    pub const fn value(&self) -> &T {
        &self.value
    }

    /// The stamp of the write being held.
    #[must_use]
    pub const fn stamp(&self) -> &S {
        &self.stamp
    }

    /// The replica that wrote what is being held.
    #[must_use]
    pub const fn replica(&self) -> ReplicaId {
        self.replica
    }
}

impl<T: Clone, S: Ord + Copy> Merge for LwwRegister<T, S> {
    type Error = Infallible;

    fn merge(&mut self, other: &Self) -> Result<(), Self::Error> {
        if (other.stamp, other.replica) > (self.stamp, self.replica) {
            self.value.clone_from(&other.value);
            self.stamp = other.stamp;
            self.replica = other.replica;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
