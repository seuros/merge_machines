//! Observed-remove set: a remove only takes out the adds it has seen.

use crate::{
    COUNT_LEN, FIELD_LEN, Merge, Reader, Refusal, ReplicaId, SHAPE_LEN, TAG_LEN, TAG_ORSET, Writer,
    fill_free, fits_free, same_elements, tagged_frame,
};

/// Bytes one tombstone record occupies: the tag's replica then its counter.
const TOMBSTONE_LEN: usize = 2 * FIELD_LEN;

/// Bytes one add record occupies: the element then the tag it came under.
const ADD_LEN: usize = FIELD_LEN + TOMBSTONE_LEN;

/// Bytes a frame spends ahead of its records: the tag, the shape, then the
/// number of records carrying an add and the number carrying a tombstone.
const HEAD_LEN: usize = TAG_LEN + SHAPE_LEN + 2 * COUNT_LEN;

/// Which add a remove is talking about.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Tag {
    /// The replica that made the add.
    pub replica: ReplicaId,
    /// That replica's counter when it made the add.
    pub counter: u64,
}

impl Tag {
    /// The tag `replica` made an add under at `counter`. The caller keeps each
    /// replica's counters unique; the set issues none. Every pair is a valid
    /// tag.
    #[must_use]
    pub const fn new(replica: ReplicaId, counter: u64) -> Self {
        Self { replica, counter }
    }
}

/// An element and the tag it was added under.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Tagged<T> {
    /// The element added.
    pub element: T,
    /// The tag that add was made under.
    pub tag: Tag,
}

impl<T> Tagged<T> {
    /// `element`, added under `tag`. Every pair is a valid add.
    #[must_use]
    pub const fn new(element: T, tag: Tag) -> Self {
        Self { element, tag }
    }
}

/// What an honoured [`OrSet::add`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adding {
    /// The add was new and is now held.
    Added,
    /// This tag already added this element; nothing changed.
    Duplicate,
}

/// What an honoured [`OrSet::remove`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Removing {
    /// Every live add of the element is tombstoned now.
    Removed,
    /// No live add of the element was held; nothing changed.
    NotHeld,
}

/// One add the set has seen, and whether a remove has tombstoned it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Seen<T> {
    add: Tagged<T>,
    removed: bool,
}

/// A set elements can leave, where concurrent adds survive a remove.
///
/// A tombstone lives on the add it removes, so the `N` slots bound the adds
/// and a remove never needs a slot.
///
/// ```
/// use merge_machines::{Adding, Merge, OrSet, Removing, Tag};
///
/// let mut left = OrSet::<u16, 8>::new();
/// assert_eq!(left.add(1, Tag::new(1, 1)), Ok(Adding::Added));
/// let mut right = left;
///
/// // One side removes what it has seen while the other adds the same element
/// // again under its own tag.
/// assert_eq!(left.remove(&1), Removing::Removed);
/// assert_eq!(right.add(1, Tag::new(2, 1)), Ok(Adding::Added));
///
/// assert_eq!(left.merge(&right), Ok(()));
/// assert!(left.contains(&1));
/// ```
#[derive(Debug, Clone, Copy, Eq)]
pub struct OrSet<T, const N: usize> {
    slots: [Option<Seen<T>>; N],
}

impl<T: Copy + PartialEq, const N: usize> OrSet<T, N> {
    /// An empty set with no add and no tombstone.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: [const { None }; N],
        }
    }

    /// Records `element` added under `tag`. The same tag adding the same
    /// element again is an honoured repeat, [`Adding::Duplicate`], tombstoned
    /// or not, and changes nothing.
    ///
    /// # Errors
    ///
    /// [`Refusal::Mismatch`] when `tag` already added another element,
    /// [`Refusal::Full`] when `tag` is new and all `N` slots are taken. Either
    /// way the set is unchanged.
    pub fn add(&mut self, element: T, tag: Tag) -> Result<Adding, Refusal> {
        if let Some(seen) = self.seen(tag) {
            return if seen.add.element == element {
                Ok(Adding::Duplicate)
            } else {
                Err(Refusal::Mismatch)
            };
        }
        let free = self.slots.iter_mut().find(|slot| slot.is_none());
        *free.ok_or(Refusal::Full)? = Some(Seen {
            add: Tagged { element, tag },
            removed: false,
        });
        Ok(Adding::Added)
    }

    /// Tombstones every add of `element` this replica has seen. With none
    /// live, the answer is [`Removing::NotHeld`] and nothing changes.
    pub fn remove(&mut self, element: &T) -> Removing {
        let mut answer = Removing::NotHeld;
        for seen in self.slots.iter_mut().flatten() {
            if seen.add.element == *element && !seen.removed {
                seen.removed = true;
                answer = Removing::Removed;
            }
        }
        answer
    }

    /// Whether `element` has an add that is not tombstoned.
    #[must_use]
    pub fn contains(&self, element: &T) -> bool {
        self.live().any(|add| add.element == *element)
    }

    /// How many distinct elements are in the set.
    #[must_use]
    pub fn len(&self) -> usize {
        self.iter().count()
    }

    /// Whether no element is live.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.live().next().is_none()
    }

    /// The elements still in the set, each once, in no particular order.
    pub fn iter(&self) -> impl Iterator<Item = T> + '_ {
        self.live().enumerate().filter_map(move |(index, add)| {
            let earlier = self
                .live()
                .take(index)
                .any(|other| other.element == add.element);
            if earlier { None } else { Some(add.element) }
        })
    }

    /// Every add this replica has seen, tombstoned or not.
    pub fn adds(&self) -> impl Iterator<Item = Tagged<T>> + '_ {
        self.slots.iter().flatten().map(|seen| seen.add)
    }

    /// Every tag this replica has tombstoned.
    pub fn removes(&self) -> impl Iterator<Item = Tag> + '_ {
        let removed = self.slots.iter().flatten().filter(|seen| seen.removed);
        removed.map(|seen| seen.add.tag)
    }

    /// The adds no remove has tombstoned.
    fn live(&self) -> impl Iterator<Item = Tagged<T>> + '_ {
        let live = self.slots.iter().flatten().filter(|seen| !seen.removed);
        live.map(|seen| seen.add)
    }

    fn seen(&self, tag: Tag) -> Option<&Seen<T>> {
        self.slots.iter().flatten().find(|seen| seen.add.tag == tag)
    }

    fn seen_mut(&mut self, tag: Tag) -> Option<&mut Seen<T>> {
        let mut slots = self.slots.iter_mut().flatten();
        slots.find(|seen| seen.add.tag == tag)
    }
}

impl<const N: usize> OrSet<u64, N> {
    /// Bytes a frame spends on its add records.
    const ADDS_LEN: usize = N * ADD_LEN;

    /// Bytes a frame spends on its tombstone records.
    const REMOVES_LEN: usize = N * TOMBSTONE_LEN;

    /// Length of the [`encode`](Self::encode) frame: the head, the add
    /// records, then the tombstone records.
    pub const ENCODED_LEN: usize = HEAD_LEN + Self::ADDS_LEN + Self::REMOVES_LEN;

    encoders!("set");

    /// Fills a frame of exactly [`ENCODED_LEN`](Self::ENCODED_LEN) bytes.
    fn write_frame(&self, frame: &mut Writer<'_>) -> Result<(), Refusal> {
        frame.byte(TAG_ORSET)?;
        frame.small(N)?;
        frame.small(self.adds().count())?;
        frame.small(self.removes().count())?;

        let (records, _) = frame.take(Self::ADDS_LEN)?.as_chunks_mut::<ADD_LEN>();
        for (record, add) in records.iter_mut().zip(self.adds()) {
            let mut record = Writer::new(record);
            record.u64(add.element)?;
            write_tag(add.tag, &mut record)?;
        }

        let (records, _) = frame
            .take(Self::REMOVES_LEN)?
            .as_chunks_mut::<TOMBSTONE_LEN>();
        for (record, tag) in records.iter_mut().zip(self.removes()) {
            write_tag(tag, &mut Writer::new(record))?;
        }
        Ok(())
    }

    /// Reads a set written by [`encode`](Self::encode).
    ///
    /// # Errors
    ///
    /// [`Refusal::Truncated`] when `bytes` is too short,
    /// [`Refusal::WrongStructure`] when it holds another structure,
    /// [`Refusal::WrongShape`] when it was written with another capacity,
    /// [`Refusal::Malformed`] when it holds more records than slots, one tag
    /// twice, or a tombstone for an add it does not hold.
    pub fn decode(bytes: &[u8]) -> Result<Self, Refusal> {
        let mut set = Self::new();
        set.decode_into(bytes)?;
        Ok(set)
    }

    /// Reads a frame over a set the caller already holds.
    ///
    /// The set is untouched when the frame is rejected.
    ///
    /// # Errors
    ///
    /// The same as [`decode`](Self::decode).
    pub fn decode_into(&mut self, bytes: &[u8]) -> Result<(), Refusal> {
        let mut frame = tagged_frame(bytes, Self::ENCODED_LEN, TAG_ORSET)?;
        if frame.small()? != N {
            return Err(Refusal::WrongShape);
        }

        let adds_held = frame.small()?;
        let removes_held = frame.small()?;
        if adds_held > N || removes_held > adds_held {
            return Err(Refusal::Malformed);
        }

        let mut set = Self::new();
        let (records, _) = frame.take(Self::ADDS_LEN)?.as_chunks::<ADD_LEN>();
        for (slot, record) in set.slots.iter_mut().zip(records.iter().take(adds_held)) {
            let mut record = Reader::new(record);
            let add = Tagged {
                element: record.u64()?,
                tag: read_tag(&mut record)?,
            };
            *slot = Some(Seen {
                add,
                removed: false,
            });
        }
        let repeated = |tag: Tag| set.adds().filter(|add| add.tag == tag).count() > 1;
        if set.adds().any(|add| repeated(add.tag)) {
            return Err(Refusal::Malformed);
        }

        let (records, _) = frame.take(Self::REMOVES_LEN)?.as_chunks::<TOMBSTONE_LEN>();
        for record in records.iter().take(removes_held) {
            match set.seen_mut(read_tag(&mut Reader::new(record))?) {
                Some(seen) if !seen.removed => seen.removed = true,
                Some(_) | None => return Err(Refusal::Malformed),
            }
        }
        *self = set;
        Ok(())
    }
}

/// Writes `tag` as the next [`TOMBSTONE_LEN`] bytes: its replica, then its
/// counter.
fn write_tag(tag: Tag, field: &mut Writer<'_>) -> Result<(), Refusal> {
    field.u64(tag.replica)?;
    field.u64(tag.counter)
}

/// Reads the next [`TOMBSTONE_LEN`] bytes as a tag.
fn read_tag(field: &mut Reader<'_>) -> Result<Tag, Refusal> {
    Ok(Tag {
        replica: field.u64()?,
        counter: field.u64()?,
    })
}

impl<T: Copy + PartialEq, const N: usize> Merge for OrSet<T, N> {
    type Error = Refusal;

    /// # Errors
    ///
    /// [`Refusal::Mismatch`] when one tag added different elements on the two
    /// sides, [`Refusal::Full`] when the adds `other` holds and this set does
    /// not outnumber the free slots. Both are checked before anything moves,
    /// so a refused merge leaves the set exactly as it was.
    fn merge(&mut self, other: &Self) -> Result<(), Self::Error> {
        let mut unseen = [false; N];
        for (mark, slot) in unseen.iter_mut().zip(&other.slots) {
            let Some(theirs) = slot else { continue };
            match self.seen(theirs.add.tag) {
                Some(mine) if mine.add.element != theirs.add.element => {
                    return Err(Refusal::Mismatch);
                }
                Some(_) => {}
                None => *mark = true,
            }
        }
        if !fits_free(&self.slots, &unseen) {
            return Err(Refusal::Full);
        }

        for theirs in other.slots.iter().flatten().filter(|seen| seen.removed) {
            if let Some(mine) = self.seen_mut(theirs.add.tag) {
                mine.removed = true;
            }
        }
        fill_free(&mut self.slots, &other.slots, &unseen);
        Ok(())
    }
}

/// Two sets are equal when they have seen the same adds and the same
/// tombstones, whatever order the slots were filled in.
impl<T: PartialEq, const N: usize> PartialEq for OrSet<T, N> {
    fn eq(&self, other: &Self) -> bool {
        same_elements(&self.slots, &other.slots)
    }
}

impl<T: Copy + PartialEq, const N: usize> Default for OrSet<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
