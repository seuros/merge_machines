//! A counter that goes both ways, from two that only go up.

#[cfg(feature = "alloc")]
use crate::boxed_frame;
use crate::{
    CapacityError, CodecError, GCounter, Merge, ReplicaId, SHAPE_LEN, TAG, TAG_LEN, TAG_PNCOUNTER,
    read_small, small_field, tagged_frame, write_into,
};

/// Where the shape sits in a frame.
const SHAPE: core::ops::Range<usize> = TAG_LEN..TAG_LEN + SHAPE_LEN;

/// Where the slot table of what went up starts in a frame.
const INCREMENTS: usize = SHAPE.end;

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
    #[must_use]
    pub const fn new() -> Self {
        Self {
            increments: GCounter::new(),
            decrements: GCounter::new(),
        }
    }

    /// # Errors
    ///
    /// Returns [`CapacityError`] when `replica` has no slot yet and all `R` are
    /// taken.
    pub fn increment(&mut self, replica: ReplicaId, by: u64) -> Result<(), CapacityError> {
        self.increments.increment(replica, by)
    }

    /// # Errors
    ///
    /// Returns [`CapacityError`] when `replica` has no slot yet and all `R` are
    /// taken.
    pub fn decrement(&mut self, replica: ReplicaId, by: u64) -> Result<(), CapacityError> {
        self.decrements.increment(replica, by)
    }

    /// Everything added, minus everything taken away.
    #[must_use]
    pub fn value(&self) -> i128 {
        i128::from(self.increments.value()) - i128::from(self.decrements.value())
    }

    /// Everything added, by every replica.
    #[must_use]
    pub fn added(&self) -> u64 {
        self.increments.value()
    }

    /// Everything taken away, by every replica.
    #[must_use]
    pub fn removed(&self) -> u64 {
        self.decrements.value()
    }

    /// Where the slot table of what came down starts in a frame.
    const DECREMENTS: usize = INCREMENTS + GCounter::<R>::BODY_LEN;

    /// Length of the [`encode`](Self::encode) frame.
    pub const ENCODED_LEN: usize = Self::DECREMENTS + GCounter::<R>::BODY_LEN;

    /// Writes the counter into `out`, returning the number of bytes written.
    ///
    /// # Errors
    ///
    /// [`CodecError::Truncated`] when `out` is shorter than
    /// [`ENCODED_LEN`](Self::ENCODED_LEN).
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        write_into(out, Self::ENCODED_LEN, |frame| self.write_frame(frame))
    }

    /// The counter's frame, exactly [`ENCODED_LEN`](Self::ENCODED_LEN) bytes.
    #[cfg(feature = "alloc")]
    #[must_use]
    pub fn encoded(&self) -> alloc::boxed::Box<[u8]> {
        boxed_frame(Self::ENCODED_LEN, |frame| self.write_frame(frame))
    }

    /// Fills a frame of exactly [`ENCODED_LEN`](Self::ENCODED_LEN) bytes.
    fn write_frame(&self, frame: &mut [u8]) {
        frame[TAG] = TAG_PNCOUNTER;
        frame[SHAPE].copy_from_slice(&small_field(R));
        self.increments
            .write_body(&mut frame[INCREMENTS..Self::DECREMENTS]);
        self.decrements.write_body(&mut frame[Self::DECREMENTS..]);
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
        let frame = tagged_frame(bytes, Self::ENCODED_LEN, TAG_PNCOUNTER)?;
        if read_small(&frame[SHAPE]) != R {
            return Err(CodecError::Parameters);
        }

        self.increments
            .read_body(&frame[INCREMENTS..Self::DECREMENTS]);
        self.decrements.read_body(&frame[Self::DECREMENTS..]);
        Ok(())
    }
}

impl<const R: usize> Merge for PNCounter<R> {
    type Error = CapacityError;

    fn merge(&mut self, other: &Self) -> Result<(), Self::Error> {
        self.increments.merge(&other.increments)?;
        self.decrements.merge(&other.decrements)
    }
}

impl<const R: usize> Default for PNCounter<R> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
