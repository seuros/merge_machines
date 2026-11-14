//! Observed-remove set: a remove only takes out the adds it has seen.

use crate::{
    COUNT_LEN, FIELD_LEN, Merge, Refusal, ReplicaId, SHAPE_LEN, TAG, TAG_LEN, TAG_ORSET, fill_free,
    fits_free, read_small, read_u64, same_elements, small_field, tagged_frame,
};

/// Bytes one tombstone record occupies: the tag's replica then its counter.
const TOMBSTONE_LEN: usize = 2 * FIELD_LEN;

/// Bytes one add record occupies: the element then the tag it came under.
const ADD_LEN: usize = FIELD_LEN + TOMBSTONE_LEN;

/// Where the element sits in an add record.
const ELEMENT: core::ops::Range<usize> = 0..FIELD_LEN;

/// Where the tag sits in an add record.
const TAGGED: core::ops::Range<usize> = FIELD_LEN..ADD_LEN;

/// Where the tag's replica sits in a tombstone record.
const REPLICA: core::ops::Range<usize> = 0..FIELD_LEN;

/// Where the tag's counter sits in a tombstone record.
const COUNTER: core::ops::Range<usize> = FIELD_LEN..TOMBSTONE_LEN;

/// Where the shape sits in a frame.
const SHAPE: core::ops::Range<usize> = TAG_LEN..TAG_LEN + SHAPE_LEN;

/// Where the number of records carrying an add sits in a frame.
const ADDS_HELD: core::ops::Range<usize> = SHAPE.end..SHAPE.end + COUNT_LEN;

/// Where the number of records carrying a tombstone sits in a frame.
const REMOVES_HELD: core::ops::Range<usize> = ADDS_HELD.end..ADDS_HELD.end + COUNT_LEN;

/// Where the add records start in a frame.
const ADDS: usize = REMOVES_HELD.end;

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

    #[must_use]
    pub fn contains(&self, element: &T) -> bool {
        self.live().any(|add| add.element == *element)
    }

    /// How many distinct elements are in the set.
    #[must_use]
    pub fn len(&self) -> usize {
        self.iter().count()
    }

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
    /// Where the tombstone records start in a frame.
    const REMOVES: usize = ADDS + N * ADD_LEN;

    /// Length of the [`encode`](Self::encode) frame.
    pub const ENCODED_LEN: usize = Self::REMOVES + N * TOMBSTONE_LEN;

    encoders!("set");

    /// Fills a frame of exactly [`ENCODED_LEN`](Self::ENCODED_LEN) bytes.
    fn write_frame(&self, frame: &mut [u8]) {
        frame.fill(0);
        frame[TAG] = TAG_ORSET;
        frame[SHAPE].copy_from_slice(&small_field(N));
        frame[ADDS_HELD].copy_from_slice(&small_field(self.adds().count()));
        frame[REMOVES_HELD].copy_from_slice(&small_field(self.removes().count()));

        let (records, _) = frame[ADDS..Self::REMOVES].as_chunks_mut::<ADD_LEN>();
        for (record, add) in records.iter_mut().zip(self.adds()) {
            record[ELEMENT].copy_from_slice(&add.element.to_le_bytes());
            write_tag(add.tag, &mut record[TAGGED]);
        }

        let (records, _) = frame[Self::REMOVES..].as_chunks_mut::<TOMBSTONE_LEN>();
        for (record, tag) in records.iter_mut().zip(self.removes()) {
            write_tag(tag, record);
        }
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
        let frame = tagged_frame(bytes, Self::ENCODED_LEN, TAG_ORSET)?;
        if read_small(&frame[SHAPE]) != N {
            return Err(Refusal::WrongShape);
        }

        let adds_held = read_small(&frame[ADDS_HELD]);
        let removes_held = read_small(&frame[REMOVES_HELD]);
        if adds_held > N || removes_held > adds_held {
            return Err(Refusal::Malformed);
        }

        let mut set = Self::new();
        let (records, _) = frame[ADDS..Self::REMOVES].as_chunks::<ADD_LEN>();
        for (slot, record) in set.slots.iter_mut().zip(records.iter().take(adds_held)) {
            let add = Tagged {
                element: read_u64(&record[ELEMENT]),
                tag: read_tag(&record[TAGGED]),
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

        let (records, _) = frame[Self::REMOVES..].as_chunks::<TOMBSTONE_LEN>();
        for record in records.iter().take(removes_held) {
            match set.seen_mut(read_tag(record)) {
                Some(seen) if !seen.removed => seen.removed = true,
                Some(_) | None => return Err(Refusal::Malformed),
            }
        }
        *self = set;
        Ok(())
    }
}

/// Writes `tag` into `field`, which is [`TOMBSTONE_LEN`] long.
fn write_tag(tag: Tag, field: &mut [u8]) {
    field[REPLICA].copy_from_slice(&tag.replica.to_le_bytes());
    field[COUNTER].copy_from_slice(&tag.counter.to_le_bytes());
}

/// Reads a tag from `field`, which is [`TOMBSTONE_LEN`] long.
fn read_tag(field: &[u8]) -> Tag {
    Tag {
        replica: read_u64(&field[REPLICA]),
        counter: read_u64(&field[COUNTER]),
    }
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
