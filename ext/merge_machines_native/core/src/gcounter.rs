//! A counter that only goes up, held as one count per replica.

use crate::{
    COUNT_LEN, FIELD_LEN, Merge, Reader, Refusal, ReplicaId, SHAPE_LEN, TAG_GCOUNTER, TAG_LEN,
    Writer, fill_free, tagged_frame,
};

/// Bytes one slot record occupies: the replica id then its count.
const RECORD_LEN: usize = 2 * FIELD_LEN;

/// Bytes a frame spends ahead of its body: the tag then the shape.
const HEAD_LEN: usize = TAG_LEN + SHAPE_LEN;

/// One replica's contribution to a [`GCounter`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// The replica that owns this count.
    pub replica: ReplicaId,
    /// How much that replica has added.
    pub count: u64,
}

impl Entry {
    /// `replica`'s contribution of `count`. Every pair is a valid entry.
    #[must_use]
    pub const fn new(replica: ReplicaId, count: u64) -> Self {
        Self { replica, count }
    }
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
    /// A counter at zero on every replica.
    #[must_use]
    pub const fn new() -> Self {
        Self { slots: [None; R] }
    }

    /// Adds `by` to `replica`'s own count. Adding zero is honoured and
    /// changes nothing: it claims no slot.
    ///
    /// # Errors
    ///
    /// [`Refusal::Full`] when `replica` has no slot yet and all `R` are taken,
    /// [`Refusal::Overflow`] when the count would pass [`u64::MAX`]. Either
    /// way the counter is unchanged.
    pub fn increment(&mut self, replica: ReplicaId, by: u64) -> Result<(), Refusal> {
        if by == 0 {
            return Ok(());
        }
        if let Some(entry) = self.held_mut(replica) {
            entry.count = entry.count.checked_add(by).ok_or(Refusal::Overflow)?;
            return Ok(());
        }
        let free = self.slots.iter_mut().find(|slot| slot.is_none());
        let slot = free.ok_or(Refusal::Full)?;
        *slot = Some(Entry { replica, count: by });
        Ok(())
    }

    /// What `replica` has added, zero if it has no slot here.
    #[must_use]
    pub fn count(&self, replica: ReplicaId) -> u64 {
        self.held(replica).map_or(0, |entry| entry.count)
    }

    /// Every replica's count, totalled wide enough never to clamp.
    #[must_use]
    pub fn value(&self) -> u128 {
        self.entries().map(|entry| u128::from(entry.count)).sum()
    }

    /// The per-replica counts, in no particular order.
    pub fn entries(&self) -> impl Iterator<Item = Entry> + '_ {
        self.slots.iter().flatten().copied()
    }

    /// Whether every replica `other` counts and this counter does not fits in
    /// a free slot here.
    pub(crate) fn fits(&self, other: &Self) -> bool {
        let unseen = other
            .entries()
            .filter(|entry| self.held(entry.replica).is_none())
            .count();
        unseen <= self.slots.iter().filter(|slot| slot.is_none()).count()
    }

    /// Folds `other` in, once [`fits`](Self::fits) has said it can land whole.
    pub(crate) fn absorb(&mut self, other: &Self) {
        let mut unseen = [false; R];
        for (mark, slot) in unseen.iter_mut().zip(&other.slots) {
            let Some(entry) = slot else { continue };
            match self.held_mut(entry.replica) {
                Some(held) => held.count = held.count.max(entry.count),
                None => *mark = true,
            }
        }
        fill_free(&mut self.slots, &other.slots, &unseen);
    }

    /// Length of the [`encode`](Self::encode) frame.
    pub const ENCODED_LEN: usize = HEAD_LEN + Self::BODY_LEN;

    /// Length of the slot table a frame carries after its tag and shape: the
    /// number of records carrying a slot, then the records.
    pub(crate) const BODY_LEN: usize = COUNT_LEN + Self::RECORDS_LEN;

    /// Bytes a slot table spends on its records.
    const RECORDS_LEN: usize = R * RECORD_LEN;

    encoders!("counter");

    /// Fills a frame of exactly [`ENCODED_LEN`](Self::ENCODED_LEN) bytes.
    fn write_frame(&self, frame: &mut Writer<'_>) -> Result<(), Refusal> {
        frame.byte(TAG_GCOUNTER)?;
        frame.small(R)?;
        self.write_body(frame)
    }

    /// Writes the [`BODY_LEN`](Self::BODY_LEN)-byte slot table into a zeroed
    /// frame, so records past the held ones stay zero.
    pub(crate) fn write_body(&self, frame: &mut Writer<'_>) -> Result<(), Refusal> {
        frame.small(self.entries().count())?;

        let (records, _) = frame.take(Self::RECORDS_LEN)?.as_chunks_mut::<RECORD_LEN>();
        for (record, entry) in records.iter_mut().zip(self.entries()) {
            let mut record = Writer::new(record);
            record.u64(entry.replica)?;
            record.u64(entry.count)?;
        }
        Ok(())
    }

    /// Reads a counter written by [`encode`](Self::encode).
    ///
    /// # Errors
    ///
    /// [`Refusal::Truncated`] when `bytes` is too short,
    /// [`Refusal::WrongStructure`] when it holds another structure,
    /// [`Refusal::WrongShape`] when it was written with another replica count,
    /// [`Refusal::Malformed`] when it holds more records than slots, one
    /// replica twice, or a zero count.
    pub fn decode(bytes: &[u8]) -> Result<Self, Refusal> {
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
    pub fn decode_into(&mut self, bytes: &[u8]) -> Result<(), Refusal> {
        let mut frame = tagged_frame(bytes, Self::ENCODED_LEN, TAG_GCOUNTER)?;
        if frame.small()? != R {
            return Err(Refusal::WrongShape);
        }

        *self = Self::read_body(&mut frame)?;
        Ok(())
    }

    /// Reads the next [`BODY_LEN`](Self::BODY_LEN)-byte slot table.
    ///
    /// # Errors
    ///
    /// [`Refusal::Malformed`] for a table no sequence of calls produces.
    pub(crate) fn read_body(frame: &mut Reader<'_>) -> Result<Self, Refusal> {
        let held = frame.small()?;
        if held > R {
            return Err(Refusal::Malformed);
        }
        let (records, _) = frame.take(Self::RECORDS_LEN)?.as_chunks::<RECORD_LEN>();

        let mut counter = Self::new();
        for (slot, record) in counter.slots.iter_mut().zip(records.iter().take(held)) {
            let mut record = Reader::new(record);
            *slot = Some(Entry {
                replica: record.u64()?,
                count: record.u64()?,
            });
        }
        let repeated = |entry: Entry| {
            let named = counter
                .entries()
                .filter(|other| other.replica == entry.replica);
            named.count() > 1
        };
        if counter
            .entries()
            .any(|entry| entry.count == 0 || repeated(entry))
        {
            return Err(Refusal::Malformed);
        }
        Ok(counter)
    }

    fn held(&self, replica: ReplicaId) -> Option<&Entry> {
        self.slots
            .iter()
            .flatten()
            .find(|entry| entry.replica == replica)
    }

    fn held_mut(&mut self, replica: ReplicaId) -> Option<&mut Entry> {
        self.slots
            .iter_mut()
            .flatten()
            .find(|entry| entry.replica == replica)
    }
}

impl<const R: usize> Merge for GCounter<R> {
    type Error = Refusal;

    /// # Errors
    ///
    /// [`Refusal::Full`] when the replicas `other` counts and this counter
    /// does not outnumber the free slots. Checked before anything moves, so a
    /// refused merge leaves the counter exactly as it was.
    fn merge(&mut self, other: &Self) -> Result<(), Self::Error> {
        if !self.fits(other) {
            return Err(Refusal::Full);
        }
        self.absorb(other);
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
