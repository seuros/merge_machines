//! Observed-remove set: a remove only takes out the adds it has seen.

use crate::{CapacityError, Merge, ReplicaId, free_slot, same_elements};

/// Which add a remove is talking about.
///
/// Unique across the cluster: the replica that made the add, and a counter from
/// that replica's [`LogicalClock`](crate::LogicalClock). Two replicas adding
/// the same element concurrently produce two different tags, so a remove that
/// only saw one of them leaves the other standing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tag {
    /// The replica that made the add.
    pub replica: ReplicaId,
    /// That replica's counter when it made the add.
    pub counter: u64,
}

impl Tag {
    #[must_use]
    pub const fn new(replica: ReplicaId, counter: u64) -> Self {
        Self { replica, counter }
    }
}

/// An element and the tag it was added under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tagged<T> {
    /// The element added.
    pub element: T,
    /// The tag that add was made under.
    pub tag: Tag,
}

/// A set elements can leave, where concurrent adds survive a remove.
///
/// Every add carries a [`Tag`]. Removing an element tombstones the tags this
/// replica has seen for it, and an element is in the set while it has at least
/// one tag that is not tombstoned. So a remove races a concurrent add on
/// another replica and loses, which is the behaviour people expect from a set
/// of live members, and re-adding after a remove works because the new tag was
/// never tombstoned.
///
/// `N` bounds both the adds and the tombstones. Removing never frees an add
/// slot: that is what lets a tombstone merge in after the add it answers.
///
/// ```
/// use merge_machines::{Merge, OrSet, Tag};
///
/// let mut left = OrSet::<u16, 8>::new();
/// assert_eq!(left.add(1, Tag::new(1, 1)), Ok(()));
/// let mut right = left;
///
/// // One side removes what it has seen while the other adds the same element
/// // again under its own tag.
/// assert_eq!(left.remove(&1), Ok(()));
/// assert_eq!(right.add(1, Tag::new(2, 1)), Ok(()));
///
/// assert_eq!(left.merge(&right), Ok(()));
/// assert!(left.contains(&1));
/// ```
#[derive(Debug, Clone, Copy, Eq)]
pub struct OrSet<T, const N: usize> {
    adds: [Option<Tagged<T>>; N],
    removes: [Option<Tag>; N],
}

impl<T: Copy + PartialEq, const N: usize> OrSet<T, N> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            adds: [const { None }; N],
            removes: [None; N],
        }
    }

    /// Records `element` added under `tag`.
    ///
    /// # Errors
    ///
    /// Returns [`CapacityError`] when `tag` is new and all `N` add slots are
    /// taken.
    pub fn add(&mut self, element: T, tag: Tag) -> Result<(), CapacityError> {
        if self.added(tag) {
            return Ok(());
        }
        let free = free_slot(&self.adds).ok_or(CapacityError)?;
        self.adds[free] = Some(Tagged { element, tag });
        Ok(())
    }

    /// Tombstones every tag this replica has seen for `element`.
    ///
    /// # Errors
    ///
    /// Returns [`CapacityError`] when the tombstones do not fit in `N` slots.
    pub fn remove(&mut self, element: &T) -> Result<(), CapacityError> {
        let mut cursor = 0;
        while cursor < N {
            if let Some(add) = self.adds[cursor]
                && add.element == *element
            {
                self.tombstone(add.tag)?;
            }
            cursor += 1;
        }
        Ok(())
    }

    #[must_use]
    pub fn contains(&self, element: &T) -> bool {
        self.adds
            .iter()
            .flatten()
            .any(|add| add.element == *element && !self.removed(add.tag))
    }

    /// How many distinct elements are in the set.
    #[must_use]
    pub fn len(&self) -> usize {
        self.iter().count()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The elements still in the set, each once, in no particular order.
    pub fn iter(&self) -> impl Iterator<Item = T> + '_ {
        self.adds
            .iter()
            .enumerate()
            .filter_map(move |(index, slot)| {
                let add = (*slot)?;
                if self.removed(add.tag) {
                    return None;
                }
                let earlier = self.adds[..index]
                    .iter()
                    .flatten()
                    .any(|other| other.element == add.element && !self.removed(other.tag));
                if earlier { None } else { Some(add.element) }
            })
    }

    /// Every add this replica has seen, tombstoned or not.
    pub fn adds(&self) -> impl Iterator<Item = Tagged<T>> + '_ {
        self.adds.iter().flatten().copied()
    }

    /// Every tag this replica has tombstoned.
    pub fn removes(&self) -> impl Iterator<Item = Tag> + '_ {
        self.removes.iter().flatten().copied()
    }

    fn added(&self, tag: Tag) -> bool {
        self.adds.iter().flatten().any(|add| add.tag == tag)
    }

    fn removed(&self, tag: Tag) -> bool {
        self.removes.iter().flatten().any(|held| *held == tag)
    }

    fn tombstone(&mut self, tag: Tag) -> Result<(), CapacityError> {
        if self.removed(tag) {
            return Ok(());
        }
        let free = free_slot(&self.removes).ok_or(CapacityError)?;
        self.removes[free] = Some(tag);
        Ok(())
    }
}

impl<T: Copy + PartialEq, const N: usize> Merge for OrSet<T, N> {
    type Error = CapacityError;

    fn merge(&mut self, other: &Self) -> Result<(), Self::Error> {
        for add in other.adds() {
            self.add(add.element, add.tag)?;
        }
        for tag in other.removes() {
            self.tombstone(tag)?;
        }
        Ok(())
    }
}

/// Two sets are equal when they have seen the same adds and the same
/// tombstones, whatever order the slots were filled in.
impl<T: PartialEq, const N: usize> PartialEq for OrSet<T, N> {
    fn eq(&self, other: &Self) -> bool {
        same_elements(&self.adds, &other.adds) && same_elements(&self.removes, &other.removes)
    }
}

impl<T: Copy + PartialEq, const N: usize> Default for OrSet<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
