//! A hybrid logical clock: timestamps near physical time that still order.

#[cfg(feature = "alloc")]
use crate::boxed_frame;
use crate::{
    ClockExhausted, CodecError, FIELD_LEN, STAMP_HYBRID, Stamp, TAG, TAG_HYBRID_CLOCK, TAG_LEN,
    read_u64, tagged_frame, write_into,
};

/// Bytes a [`HybridTimestamp`] occupies in a frame.
const TIMESTAMP_LEN: usize = 2 * FIELD_LEN;

/// Where the wall-clock reading sits in a timestamp field.
const WALL: core::ops::Range<usize> = 0..FIELD_LEN;

/// Where the counter sits in a timestamp field.
const COUNTER: core::ops::Range<usize> = FIELD_LEN..TIMESTAMP_LEN;

/// Where the last timestamp issued sits in a clock frame.
const LAST: core::ops::Range<usize> = TAG_LEN..TAG_LEN + TIMESTAMP_LEN;

/// A wall-clock reading paired with a counter that breaks its ties.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct HybridTimestamp {
    /// The wall-clock reading this was issued at, in nanoseconds.
    pub wall_ns: u64,
    /// How many timestamps have been issued at this reading.
    pub counter: u64,
}

impl Stamp for HybridTimestamp {
    const KIND: u8 = STAMP_HYBRID;
    const ENCODED_LEN: usize = TIMESTAMP_LEN;

    fn write(self, field: &mut [u8]) {
        field[WALL].copy_from_slice(&self.wall_ns.to_le_bytes());
        field[COUNTER].copy_from_slice(&self.counter.to_le_bytes());
    }

    fn read(field: &[u8]) -> Self {
        Self {
            wall_ns: read_u64(&field[WALL]),
            counter: read_u64(&field[COUNTER]),
        }
    }
}

/// Issues timestamps that track physical time without ever going backwards.
///
/// # The reading this wants
///
/// `wall_ns` is physical time that two machines can compare: nanoseconds since
/// the Unix epoch. It is allowed to jump backwards, which is exactly why the
/// counter exists.
///
/// It is not a monotonic reading, and nothing derived from it is a deadline, a
/// timeout or a budget. Measuring how long something took needs the monotonic
/// clock, which this crate neither reads nor models. Wiring a monotonic reading
/// in here produces timestamps that no other machine can order against its own.
///
/// ```
/// use merge_machines::{ClockExhausted, HybridClock, HybridTimestamp};
///
/// const MS: u64 = 1_000_000;
/// let mut clock = HybridClock::new();
///
/// let first = clock.tick(10 * MS)?;
/// assert_eq!(first.counter, 0);
///
/// // Same reading twice: the counter separates them.
/// let second = clock.tick(10 * MS)?;
/// assert!(second > first);
///
/// // The wall clock is corrected backwards; the timestamps are not.
/// let third = clock.tick(4 * MS)?;
/// assert!(third > second);
/// assert_eq!(third.wall_ns, 10 * MS);
///
/// // A reading that really did advance resets the counter.
/// let fourth = clock.tick(11 * MS)?;
/// assert_eq!(fourth.counter, 0);
///
/// // A counter with nothing left reports it instead of repeating itself.
/// let crowded = HybridTimestamp { wall_ns: 11 * MS, counter: u64::MAX };
/// assert_eq!(clock.observe(0, crowded), Err(ClockExhausted));
/// assert_eq!(clock.last(), fourth);
/// # Ok::<(), ClockExhausted>(())
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct HybridClock {
    last: HybridTimestamp,
}

const fn larger(left: u64, right: u64) -> u64 {
    if left > right { left } else { right }
}

impl HybridClock {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            last: HybridTimestamp {
                wall_ns: 0,
                counter: 0,
            },
        }
    }

    /// The last timestamp this clock issued.
    #[must_use]
    pub const fn last(&self) -> HybridTimestamp {
        self.last
    }

    /// Stamps a local event at wall-clock reading `wall_ns`.
    ///
    /// # Errors
    ///
    /// [`ClockExhausted`] when the reading has not advanced and the counter
    /// that separates two events at the same reading holds its highest value,
    /// which leaves the clock where it was.
    pub const fn tick(&mut self, wall_ns: u64) -> Result<HybridTimestamp, ClockExhausted> {
        if wall_ns > self.last.wall_ns {
            self.last = HybridTimestamp {
                wall_ns,
                counter: 0,
            };
            return Ok(self.last);
        }
        let Some(counter) = self.last.counter.checked_add(1) else {
            return Err(ClockExhausted);
        };
        self.last.counter = counter;
        Ok(self.last)
    }

    /// Stamps the receipt of `observed` at wall-clock reading `wall_ns`.
    ///
    /// # Errors
    ///
    /// The same as [`tick`](Self::tick), and the observed timestamp is not
    /// adopted either.
    pub const fn observe(
        &mut self,
        wall_ns: u64,
        observed: HybridTimestamp,
    ) -> Result<HybridTimestamp, ClockExhausted> {
        let latest = larger(wall_ns, larger(self.last.wall_ns, observed.wall_ns));
        let counter = if latest == self.last.wall_ns && latest == observed.wall_ns {
            larger(self.last.counter, observed.counter).checked_add(1)
        } else if latest == self.last.wall_ns {
            self.last.counter.checked_add(1)
        } else if latest == observed.wall_ns {
            observed.counter.checked_add(1)
        } else {
            Some(0)
        };
        let Some(counter) = counter else {
            return Err(ClockExhausted);
        };

        self.last = HybridTimestamp {
            wall_ns: latest,
            counter,
        };
        Ok(self.last)
    }

    /// Length of the [`encode`](Self::encode) frame.
    pub const ENCODED_LEN: usize = TAG_LEN + TIMESTAMP_LEN;

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
    fn write_frame(&self, frame: &mut [u8]) {
        frame[TAG] = TAG_HYBRID_CLOCK;
        self.last.write(&mut frame[LAST]);
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
        let frame = tagged_frame(bytes, Self::ENCODED_LEN, TAG_HYBRID_CLOCK)?;
        self.last = HybridTimestamp::read(&frame[LAST]);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
