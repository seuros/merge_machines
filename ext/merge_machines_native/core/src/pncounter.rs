//! A counter that goes both ways, from two that only go up.

use crate::{
    GCounter, Merge, Refusal, ReplicaId, SHAPE_LEN, TAG_LEN, TAG_PNCOUNTER, Writer, tagged_frame,
};

/// Bytes a frame spends ahead of its two slot tables: the tag then the shape.
const HEAD_LEN: usize = TAG_LEN + SHAPE_LEN;

/// A counter replicas can both raise and lower.
///
/// ```
/// use merge_machines::{Merge, PNCounter};
///
/// let mut left = PNCounter::<4>::new();
/// let mut right = PNCounter::<4>::new();
///
/// assert_eq!(left.increment(1, 10), Ok(()));
/// assert_eq!(right.decrement(2, 4), Ok(()));
///
/// assert_eq!(left.merge(&right), Ok(()));
/// assert_eq!(left.value(), 6);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PNCounter<const R: usize> {
    increments: GCounter<R>,
    decrements: GCounter<R>,
}

impl<const R: usize> PNCounter<R> {
    /// A counter at zero on every replica.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            increments: GCounter::new(),
            decrements: GCounter::new(),
        }
    }

    /// Adds `by` to what `replica` has added. Zero is honoured and changes
    /// nothing.
    ///
    /// # Errors
    ///
    /// [`Refusal::Full`] when `replica` has no slot on this side yet and all
    /// `R` are taken, [`Refusal::Overflow`] when its count would pass
    /// [`u64::MAX`]. Either way the counter is unchanged.
    pub fn increment(&mut self, replica: ReplicaId, by: u64) -> Result<(), Refusal> {
        self.increments.increment(replica, by)
    }

    /// Adds `by` to what `replica` has taken away. Zero is honoured and
    /// changes nothing.
    ///
    /// # Errors
    ///
    /// The same as [`increment`](Self::increment), on the side that counts
    /// what came down.
    pub fn decrement(&mut self, replica: ReplicaId, by: u64) -> Result<(), Refusal> {
        self.decrements.increment(replica, by)
    }

    /// Everything added, minus everything taken away.
    #[must_use]
    pub fn value(&self) -> i128 {
        // Each side totals at most `R` counts below 2^64, far below 2^127
        // for any `R` that fits in memory, so the difference of two
        // nonnegative totals cannot overflow.
        total(&self.increments) - total(&self.decrements)
    }

    /// Everything added, by every replica.
    #[must_use]
    pub fn added(&self) -> u128 {
        self.increments.value()
    }

    /// Everything taken away, by every replica.
    #[must_use]
    pub fn removed(&self) -> u128 {
        self.decrements.value()
    }

    /// Length of the [`encode`](Self::encode) frame: the head, then the slot
    /// table of what went up, then the one of what came down.
    pub const ENCODED_LEN: usize = HEAD_LEN + 2 * GCounter::<R>::BODY_LEN;

    encoders!("counter");

    /// Fills a frame of exactly [`ENCODED_LEN`](Self::ENCODED_LEN) bytes.
    fn write_frame(&self, frame: &mut Writer<'_>) -> Result<(), Refusal> {
        frame.byte(TAG_PNCOUNTER)?;
        frame.small(R)?;
        self.increments.write_body(frame)?;
        self.decrements.write_body(frame)
    }

    /// Reads a counter written by [`encode`](Self::encode).
    ///
    /// # Errors
    ///
    /// [`Refusal::Truncated`] when `bytes` is too short,
    /// [`Refusal::WrongStructure`] when it holds another structure,
    /// [`Refusal::WrongShape`] when it was written with another replica count,
    /// [`Refusal::Malformed`] when either side's table is one no sequence of
    /// calls produces.
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
        let mut frame = tagged_frame(bytes, Self::ENCODED_LEN, TAG_PNCOUNTER)?;
        if frame.small()? != R {
            return Err(Refusal::WrongShape);
        }

        let increments = GCounter::read_body(&mut frame)?;
        let decrements = GCounter::read_body(&mut frame)?;
        *self = Self {
            increments,
            decrements,
        };
        Ok(())
    }
}

/// One side's count, signed so the other side can be taken from it.
fn total<const R: usize>(side: &GCounter<R>) -> i128 {
    side.entries().map(|entry| i128::from(entry.count)).sum()
}

impl<const R: usize> Merge for PNCounter<R> {
    type Error = Refusal;

    /// # Errors
    ///
    /// [`Refusal::Full`] when either side would need more slots than it has
    /// free. Both sides are checked before either moves, so a refused merge
    /// leaves the counter exactly as it was.
    fn merge(&mut self, other: &Self) -> Result<(), Self::Error> {
        if !(self.increments.fits(&other.increments) && self.decrements.fits(&other.decrements)) {
            return Err(Refusal::Full);
        }
        self.increments.absorb(&other.increments);
        self.decrements.absorb(&other.decrements);
        Ok(())
    }
}

impl<const R: usize> Default for PNCounter<R> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
