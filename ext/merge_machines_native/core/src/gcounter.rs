//! A counter that only goes up, held as one count per replica.

use crate::{
    COUNT_LEN, CapacityError, CodecError, FIELD_LEN, Merge, ReplicaId, SHAPE_LEN, TAG,
    TAG_GCOUNTER, TAG_LEN, free_slot, read_small, read_u64, small_field,
};

/// Bytes one slot record occupies: the replica id then its count.
const RECORD_LEN: usize = 2 * FIELD_LEN;

/// Where the replica id sits in a slot record.
const REPLICA: core::ops::Range<usize> = 0..FIELD_LEN;

/// Where the count sits in a slot record.
const COUNT: core::ops::Range<usize> = FIELD_LEN..RECORD_LEN;

/// Where the number of records carrying a slot sits in a body.
const HELD: core::ops::Range<usize> = 0..COUNT_LEN;

/// Where the slot records start in a body.
const RECORDS: usize = HELD.end;

/// Where the shape sits in a frame.
const SHAPE: core::ops::Range<usize> = TAG_LEN..TAG_LEN + SHAPE_LEN;

/// Where the body starts in a frame.
const BODY: usize = SHAPE.end;

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

    /// Length of the [`encode`](Self::encode) frame.
    pub const ENCODED_LEN: usize = BODY + Self::BODY_LEN;

    /// Length of the slot table a frame carries after its tag and shape.
    pub(crate) const BODY_LEN: usize = RECORDS + R * RECORD_LEN;

    /// Writes the counter into `out`, returning the number of bytes written.
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

    /// The counter's frame, exactly [`ENCODED_LEN`](Self::ENCODED_LEN) bytes.
    #[cfg(feature = "alloc")]
    #[must_use]
    pub fn encoded(&self) -> alloc::boxed::Box<[u8]> {
        let mut frame = alloc::vec![0_u8; Self::ENCODED_LEN].into_boxed_slice();
        self.write_frame(&mut frame);
        frame
    }

    /// Fills a frame of exactly [`ENCODED_LEN`](Self::ENCODED_LEN) bytes.
    fn write_frame(&self, frame: &mut [u8]) {
        frame[TAG] = TAG_GCOUNTER;
        frame[SHAPE].copy_from_slice(&small_field(R));
        self.write_body(&mut frame[BODY..]);
    }

    /// Fills a slot table of exactly [`BODY_LEN`](Self::BODY_LEN) bytes.
    pub(crate) fn write_body(&self, body: &mut [u8]) {
        body.fill(0);
        body[HELD].copy_from_slice(&small_field(self.entries().count()));

        let (records, _) = body[RECORDS..].as_chunks_mut::<RECORD_LEN>();
        for (record, entry) in records.iter_mut().zip(self.entries()) {
            record[REPLICA].copy_from_slice(&entry.replica.to_le_bytes());
            record[COUNT].copy_from_slice(&entry.count.to_le_bytes());
        }
    }

    /// Reads a counter written by [`encode`](Self::encode).
    ///
    /// # Errors
    ///
    /// [`CodecError::Truncated`] when `bytes` is too short, [`CodecError::Tag`]
    /// when it holds another structure, [`CodecError::Parameters`] when it was
    /// written with another replica count.
    pub fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        let mut counter = Self::new();
        counter.decode_into(bytes)?;
        Ok(counter)
    }

    /// Reads a frame over a counter the caller already holds.
    ///
    /// The counter is untouched when the frame is rejected.
    ///
    /// # Errors
    ///
    /// The same as [`decode`](Self::decode).
    pub fn decode_into(&mut self, bytes: &[u8]) -> Result<(), CodecError> {
        let Some(frame) = bytes.get(..Self::ENCODED_LEN) else {
            return Err(CodecError::Truncated);
        };
        if frame[TAG] != TAG_GCOUNTER {
            return Err(CodecError::Tag);
        }
        if read_small(&frame[SHAPE]) != R {
            return Err(CodecError::Parameters);
        }

        self.read_body(&frame[BODY..]);
        Ok(())
    }

    /// Reads a slot table of exactly [`BODY_LEN`](Self::BODY_LEN) bytes.
    pub(crate) fn read_body(&mut self, body: &[u8]) {
        let held = read_small(&body[HELD]);
        let (records, _) = body[RECORDS..].as_chunks::<RECORD_LEN>();

        self.slots = [None; R];
        for (slot, record) in self.slots.iter_mut().zip(records.iter().take(held)) {
            *slot = Some(Entry {
                replica: read_u64(&record[REPLICA]),
                count: read_u64(&record[COUNT]),
            });
        }
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
