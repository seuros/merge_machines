//! A set that only grows.

use crate::{
    COUNT_LEN, FIELD_LEN, Merge, Refusal, SHAPE_LEN, TAG_GSET, TAG_LEN, Writer, fill_free,
    fits_free, same_elements, tagged_frame,
};

/// Bytes a frame spends ahead of its element records: the tag, the shape and
/// the number of records carrying an element, in that order.
const HEAD_LEN: usize = TAG_LEN + SHAPE_LEN + COUNT_LEN;

/// What an honoured [`GSet::insert`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inserting {
    /// The element was not in the set and now is.
    Inserted,
    /// The element was already in the set; nothing changed.
    Duplicate,
}

/// A set nothing is ever taken out of.
///
/// ```
/// use merge_machines::{GSet, Inserting, Merge};
///
/// let mut left = GSet::<u16, 8>::new();
/// let mut right = GSet::<u16, 8>::new();
///
/// assert_eq!(left.insert(1), Ok(Inserting::Inserted));
/// assert_eq!(left.insert(1), Ok(Inserting::Duplicate));
/// assert_eq!(right.insert(2), Ok(Inserting::Inserted));
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
    /// An empty set.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: [const { None }; N],
        }
    }

    /// Adds `element`. An element already in is an honoured repeat,
    /// [`Inserting::Duplicate`], and changes nothing.
    ///
    /// # Errors
    ///
    /// [`Refusal::Full`] when `element` is new and all `N` slots are taken;
    /// the set is unchanged.
    pub fn insert(&mut self, element: T) -> Result<Inserting, Refusal> {
        if self.contains(&element) {
            return Ok(Inserting::Duplicate);
        }
        let free = self.slots.iter_mut().find(|slot| slot.is_none());
        *free.ok_or(Refusal::Full)? = Some(element);
        Ok(Inserting::Inserted)
    }

    /// Whether `element` was ever inserted.
    #[must_use]
    pub fn contains(&self, element: &T) -> bool {
        self.slots.iter().flatten().any(|held| held == element)
    }

    /// How many elements the set holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    /// Whether the set holds no element.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The elements, in no particular order.
    pub fn iter(&self) -> impl Iterator<Item = T> + '_ {
        self.slots.iter().flatten().copied()
    }
}

impl<const N: usize> GSet<u64, N> {
    /// Length of the [`encode`](Self::encode) frame.
    pub const ENCODED_LEN: usize = HEAD_LEN + Self::RECORDS_LEN;

    /// Bytes a frame spends on its element records.
    const RECORDS_LEN: usize = N * FIELD_LEN;

    encoders!("set");

    /// Fills a frame of exactly [`ENCODED_LEN`](Self::ENCODED_LEN) bytes.
    fn write_frame(&self, frame: &mut Writer<'_>) -> Result<(), Refusal> {
        frame.byte(TAG_GSET)?;
        frame.small(N)?;
        frame.small(self.len())?;

        let (records, _) = frame.take(Self::RECORDS_LEN)?.as_chunks_mut::<FIELD_LEN>();
        for (record, element) in records.iter_mut().zip(self.iter()) {
            *record = element.to_le_bytes();
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
    /// [`Refusal::Malformed`] when it holds more records than slots or one
    /// element twice.
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
        let mut frame = tagged_frame(bytes, Self::ENCODED_LEN, TAG_GSET)?;
        if frame.small()? != N {
            return Err(Refusal::WrongShape);
        }

        let held = frame.small()?;
        if held > N {
            return Err(Refusal::Malformed);
        }
        let (records, _) = frame.take(Self::RECORDS_LEN)?.as_chunks::<FIELD_LEN>();

        let mut set = Self::new();
        for (slot, record) in set.slots.iter_mut().zip(records.iter().take(held)) {
            *slot = Some(u64::from_le_bytes(*record));
        }
        let repeated = |element: u64| set.iter().filter(|held| *held == element).count() > 1;
        if set.iter().any(repeated) {
            return Err(Refusal::Malformed);
        }
        *self = set;
        Ok(())
    }
}

impl<T: Copy + PartialEq, const N: usize> Merge for GSet<T, N> {
    type Error = Refusal;

    /// # Errors
    ///
    /// [`Refusal::Full`] when the elements `other` holds and this set does not
    /// outnumber the free slots. Checked before anything moves, so a refused
    /// merge leaves the set exactly as it was.
    fn merge(&mut self, other: &Self) -> Result<(), Self::Error> {
        let mut unseen = [false; N];
        for (mark, slot) in unseen.iter_mut().zip(&other.slots) {
            *mark = slot.is_some_and(|element| !self.contains(&element));
        }
        if !fits_free(&self.slots, &unseen) {
            return Err(Refusal::Full);
        }

        fill_free(&mut self.slots, &other.slots, &unseen);
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
