//! A set that only grows.

use crate::{
    COUNT_LEN, CapacityError, CodecError, FIELD_LEN, Merge, SHAPE_LEN, TAG, TAG_GSET, TAG_LEN,
    free_slot, read_small, read_u64, same_elements, small_field,
};

/// Where the shape sits in a frame.
const SHAPE: core::ops::Range<usize> = TAG_LEN..TAG_LEN + SHAPE_LEN;

/// Where the number of records carrying an element sits in a frame.
const HELD: core::ops::Range<usize> = SHAPE.end..SHAPE.end + COUNT_LEN;

/// Where the element records start in a frame.
const RECORDS: usize = HELD.end;

/// A set nothing is ever taken out of.
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

impl<const N: usize> GSet<u64, N> {
    /// Length of the [`encode`](Self::encode) frame.
    pub const ENCODED_LEN: usize = RECORDS + N * FIELD_LEN;

    /// Writes the set into `out`, returning the number of bytes written.
    ///
    /// # Errors
    ///
    /// [`CodecError::Truncated`] when `out` is shorter than
    /// [`ENCODED_LEN`](Self::ENCODED_LEN).
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        let Some(frame) = out.get_mut(..Self::ENCODED_LEN) else {
            return Err(CodecError::Truncated);
        };
        self.write_frame(frame);
        Ok(Self::ENCODED_LEN)
    }

    /// The set's frame, exactly [`ENCODED_LEN`](Self::ENCODED_LEN) bytes.
    #[cfg(feature = "alloc")]
    #[must_use]
    pub fn encoded(&self) -> alloc::boxed::Box<[u8]> {
        let mut frame = alloc::vec![0_u8; Self::ENCODED_LEN].into_boxed_slice();
        self.write_frame(&mut frame);
        frame
    }

    /// Fills a frame of exactly [`ENCODED_LEN`](Self::ENCODED_LEN) bytes.
    fn write_frame(&self, frame: &mut [u8]) {
        frame.fill(0);
        frame[TAG] = TAG_GSET;
        frame[SHAPE].copy_from_slice(&small_field(N));
        frame[HELD].copy_from_slice(&small_field(self.len()));

        let (records, _) = frame[RECORDS..].as_chunks_mut::<FIELD_LEN>();
        for (record, element) in records.iter_mut().zip(self.iter()) {
            *record = element.to_le_bytes();
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
        let Some(frame) = bytes.get(..Self::ENCODED_LEN) else {
            return Err(CodecError::Truncated);
        };
        if frame[TAG] != TAG_GSET {
            return Err(CodecError::Tag);
        }
        if read_small(&frame[SHAPE]) != N {
            return Err(CodecError::Parameters);
        }

        let held = read_small(&frame[HELD]);
        let (records, _) = frame[RECORDS..].as_chunks::<FIELD_LEN>();

        self.slots = [None; N];
        for (slot, record) in self.slots.iter_mut().zip(records.iter().take(held)) {
            *slot = Some(read_u64(record));
        }
        Ok(())
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
