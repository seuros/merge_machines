//! A set that only grows.

use crate::{CapacityError, Merge, free_slot, same_elements};

/// A set nothing is ever taken out of.
///
/// Adding is the only update, so merging is the union and there is nothing to
/// resolve. Use [`OrSet`](crate::OrSet) when elements have to leave.
///
/// `N` is how many distinct elements this state holds. Lookups scan the slots,
/// which is the trade for needing no heap, no hashing and no ordering of
/// elements.
///
/// ```
/// use merge_machines::{GSet, Merge};
///
/// let mut left = GSet::<u16, 8>::new();
/// let mut right = GSet::<u16, 8>::new();
///
/// assert_eq!(left.insert(1), Ok(()));
/// assert_eq!(left.insert(1), Ok(()));
/// assert_eq!(right.insert(2), Ok(()));
///
/// assert_eq!(left.merge(&right), Ok(()));
/// assert_eq!(left.len(), 2);
/// assert!(left.contains(&2));
/// ```
#[derive(Debug, Clone, Copy, Eq)]
pub struct GSet<T, const N: usize> {
    slots: [Option<T>; N],
}

impl<T: Copy + PartialEq, const N: usize> GSet<T, N> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: [const { None }; N],
        }
    }

    /// Adds `element`, which is a no-op if it is already in.
    ///
    /// # Errors
    ///
    /// Returns [`CapacityError`] when `element` is new and all `N` slots are
    /// taken.
    pub fn insert(&mut self, element: T) -> Result<(), CapacityError> {
        if self.contains(&element) {
            return Ok(());
        }
        let free = free_slot(&self.slots).ok_or(CapacityError)?;
        self.slots[free] = Some(element);
        Ok(())
    }

    #[must_use]
    pub fn contains(&self, element: &T) -> bool {
        self.slots.iter().flatten().any(|held| held == element)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The elements, in no particular order.
    pub fn iter(&self) -> impl Iterator<Item = T> + '_ {
        self.slots.iter().flatten().copied()
    }
}

impl<T: Copy + PartialEq, const N: usize> Merge for GSet<T, N> {
    type Error = CapacityError;

    fn merge(&mut self, other: &Self) -> Result<(), Self::Error> {
        for element in other.iter() {
            self.insert(element)?;
        }
        Ok(())
    }
}

/// Two sets are equal when they hold the same elements, whatever order the
/// slots were filled in.
impl<T: PartialEq, const N: usize> PartialEq for GSet<T, N> {
    fn eq(&self, other: &Self) -> bool {
        same_elements(&self.slots, &other.slots)
    }
}

impl<T: Copy + PartialEq, const N: usize> Default for GSet<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
