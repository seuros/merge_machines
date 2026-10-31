//! Lamport's logical clock: ordering events without reading a clock.

#[cfg(feature = "alloc")]
use crate::boxed_frame;
use crate::{
    ClockExhausted, CodecError, FIELD_LEN, TAG, TAG_LEN, TAG_LOGICAL_CLOCK, read_u64, tagged_frame,
    write_into,
};

/// Where the counter sits in a frame.
const COUNTER: core::ops::Range<usize> = TAG_LEN..TAG_LEN + FIELD_LEN;

/// A counter that orders events across replicas, with no clock involved.
///
/// ```
/// use merge_machines::{ClockExhausted, LogicalClock};
///
/// let mut sender = LogicalClock::new();
/// let mut receiver = LogicalClock::new();
///
/// let sent_at = sender.tick()?;
/// assert_eq!(sent_at, 1);
///
/// receiver.tick()?;
/// receiver.tick()?;
///
/// // Receiving is stamped after both the local history and the send.
/// assert_eq!(receiver.observe(sent_at), Ok(3));
/// assert_eq!(receiver.observe(99), Ok(100));
///
/// // A counter with nothing left reports it instead of repeating itself.
/// assert_eq!(receiver.observe(u64::MAX), Err(ClockExhausted));
/// assert_eq!(receiver.now(), 100);
/// # Ok::<(), ClockExhausted>(())
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LogicalClock {
    counter: u64,
}

impl LogicalClock {
    #[must_use]
    pub const fn new() -> Self {
        Self { counter: 0 }
    }

    /// The last timestamp this clock issued.
    #[must_use]
    pub const fn now(&self) -> u64 {
        self.counter
    }

    /// Stamps a local event.
    ///
    /// # Errors
    ///
    /// [`ClockExhausted`] when the counter holds its highest value, which
    /// leaves the clock where it was.
    pub const fn tick(&mut self) -> Result<u64, ClockExhausted> {
        self.issue(self.counter)
    }

    /// Stamps the receipt of `observed`, a timestamp from another replica.
    ///
    /// # Errors
    ///
    /// The same as [`tick`](Self::tick), and the observed timestamp is not
    /// adopted either.
    pub const fn observe(&mut self, observed: u64) -> Result<u64, ClockExhausted> {
        self.issue(if observed > self.counter {
            observed
        } else {
            self.counter
        })
    }

    /// Issues the timestamp that follows `base`.
    const fn issue(&mut self, base: u64) -> Result<u64, ClockExhausted> {
        let Some(next) = base.checked_add(1) else {
            return Err(ClockExhausted);
        };
        self.counter = next;
        Ok(next)
    }

    /// Length of the [`encode`](Self::encode) frame.
    pub const ENCODED_LEN: usize = TAG_LEN + FIELD_LEN;

    /// Writes the clock into `out`, returning the number of bytes written.
    ///
    /// # Errors
    ///
    /// [`CodecError::Truncated`] when `out` is shorter than
    /// [`ENCODED_LEN`](Self::ENCODED_LEN).
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        write_into(out, Self::ENCODED_LEN, |frame| self.write_frame(frame))
    }

    /// The clock's frame, exactly [`ENCODED_LEN`](Self::ENCODED_LEN) bytes.
    #[cfg(feature = "alloc")]
    #[must_use]
    pub fn encoded(&self) -> alloc::boxed::Box<[u8]> {
        boxed_frame(Self::ENCODED_LEN, |frame| self.write_frame(frame))
    }

    /// Fills a frame of exactly [`ENCODED_LEN`](Self::ENCODED_LEN) bytes.
    fn write_frame(self, frame: &mut [u8]) {
        frame[TAG] = TAG_LOGICAL_CLOCK;
        frame[COUNTER].copy_from_slice(&self.counter.to_le_bytes());
    }

    /// Reads a clock written by [`encode`](Self::encode).
    ///
    /// # Errors
    ///
    /// [`CodecError::Truncated`] when `bytes` is too short, [`CodecError::Tag`]
    /// when it holds another structure.
    pub fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        let mut clock = Self::new();
        clock.decode_into(bytes)?;
        Ok(clock)
    }

    /// Reads a frame over a clock the caller already holds.
    ///
    /// The clock is untouched when the frame is rejected.
    ///
    /// # Errors
    ///
    /// The same as [`decode`](Self::decode).
    pub fn decode_into(&mut self, bytes: &[u8]) -> Result<(), CodecError> {
        let frame = tagged_frame(bytes, Self::ENCODED_LEN, TAG_LOGICAL_CLOCK)?;
        self.counter = read_u64(&frame[COUNTER]);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
