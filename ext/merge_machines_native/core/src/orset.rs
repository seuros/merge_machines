//! Observed-remove set: a remove only takes out the adds it has seen.

#[cfg(feature = "alloc")]
use crate::boxed_frame;
use crate::{
    COUNT_LEN, CapacityError, CodecError, FIELD_LEN, Merge, ReplicaId, SHAPE_LEN, TAG, TAG_LEN,
    TAG_ORSET, free_slot, read_small, read_u64, same_elements, small_field, tagged_frame,
    write_into,
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

impl<const N: usize> OrSet<u64, N> {
    /// Where the tombstone records start in a frame.
    const REMOVES: usize = ADDS + N * ADD_LEN;

    /// Length of the [`encode`](Self::encode) frame.
    pub const ENCODED_LEN: usize = Self::REMOVES + N * TOMBSTONE_LEN;

    /// Writes the set into `out`, returning the number of bytes written.
    ///
    /// # Errors
    ///
    /// [`CodecError::Truncated`] when `out` is shorter than
    /// [`ENCODED_LEN`](Self::ENCODED_LEN).
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        write_into(out, Self::ENCODED_LEN, |frame| self.write_frame(frame))
    }

    /// The set's frame, exactly [`ENCODED_LEN`](Self::ENCODED_LEN) bytes.
    #[cfg(feature = "alloc")]
    #[must_use]
    pub fn encoded(&self) -> alloc::boxed::Box<[u8]> {
        boxed_frame(Self::ENCODED_LEN, |frame| self.write_frame(frame))
    }

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
    /// [`CodecError::Truncated`] when `bytes` is too short, [`CodecError::Tag`]
    /// when it holds another structure, [`CodecError::Parameters`] when it was
    /// written with another capacity.
    pub fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
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
    pub fn decode_into(&mut self, bytes: &[u8]) -> Result<(), CodecError> {
        let frame = tagged_frame(bytes, Self::ENCODED_LEN, TAG_ORSET)?;
        if read_small(&frame[SHAPE]) != N {
            return Err(CodecError::Parameters);
        }

        let adds_held = read_small(&frame[ADDS_HELD]);
        let removes_held = read_small(&frame[REMOVES_HELD]);

        let (records, _) = frame[ADDS..Self::REMOVES].as_chunks::<ADD_LEN>();
        self.adds = [const { None }; N];
        for (slot, record) in self.adds.iter_mut().zip(records.iter().take(adds_held)) {
            *slot = Some(Tagged {
                element: read_u64(&record[ELEMENT]),
                tag: read_tag(&record[TAGGED]),
            });
        }

        let (records, _) = frame[Self::REMOVES..].as_chunks::<TOMBSTONE_LEN>();
        self.removes = [None; N];
        for (slot, record) in self
            .removes
            .iter_mut()
            .zip(records.iter().take(removes_held))
        {
            *slot = Some(read_tag(record));
        }
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
